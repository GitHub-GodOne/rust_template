//! Let's Encrypt (ACME) issuance with DNS-01 challenges answered through
//! Cloudflare, so certificates work for any port and for wildcard names.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, Order, OrderStatus, RetryPolicy,
};

use super::cloudflare::CloudflareApi;

/// Paths and metadata of a freshly issued certificate.
#[derive(Clone, Debug)]
pub struct IssuedCertificate {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub not_before: DateTime<FixedOffset>,
    pub not_after: DateTime<FixedOffset>,
    pub issuer: String,
}

pub struct IssueRequest<'a> {
    pub names: &'a [String],
    pub directory: &'a str,
    pub contact_email: &'a str,
    pub storage_dir: &'a Path,
    pub propagation_timeout: Duration,
}

/// ACME directory URL for the configured setting.
#[must_use]
pub fn directory_url(directory: &str) -> String {
    match directory.trim() {
        "" | "production" => LetsEncrypt::Production.url().to_string(),
        "staging" => LetsEncrypt::Staging.url().to_string(),
        custom => custom.to_string(),
    }
}

/// File-system safe folder name for a certificate's primary domain.
#[must_use]
pub fn storage_name(domain: &str) -> String {
    domain
        .trim()
        .to_ascii_lowercase()
        .replace('*', "_wildcard")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Name of the TXT record that answers the challenge for `name`.
#[must_use]
pub fn challenge_record_name(name: &str) -> String {
    format!("_acme-challenge.{}", name.trim_start_matches("*."))
}

async fn account(request: &IssueRequest<'_>) -> Result<Account, String> {
    let directory = directory_url(request.directory);
    let account_file = request.storage_dir.join(format!(
        "acme-account-{}.json",
        storage_name(&directory.replace("https://", "").replace('/', "_"))
    ));
    if let Ok(raw) = tokio::fs::read_to_string(&account_file).await {
        if let Ok(credentials) = serde_json::from_str::<AccountCredentials>(&raw) {
            return Account::builder()
                .map_err(|error| format!("初始化 ACME 客户端失败：{error}"))?
                .from_credentials(credentials)
                .await
                .map_err(|error| format!("恢复 ACME 账号失败：{error}"));
        }
    }
    let contact = (!request.contact_email.trim().is_empty())
        .then(|| format!("mailto:{}", request.contact_email.trim()));
    let contacts: Vec<&str> = contact.iter().map(String::as_str).collect();
    let (account, credentials) = Account::builder()
        .map_err(|error| format!("初始化 ACME 客户端失败：{error}"))?
        .create(
            &NewAccount {
                contact: &contacts,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory,
            None,
        )
        .await
        .map_err(|error| format!("注册 ACME 账号失败：{error}"))?;
    let raw = serde_json::to_string_pretty(&credentials)
        .map_err(|error| format!("保存 ACME 账号失败：{error}"))?;
    write_private_file(&account_file, raw.as_bytes()).await?;
    Ok(account)
}

/// Issues a certificate for `request.names` (the first one is the primary name).
pub async fn issue(
    cloudflare: &CloudflareApi,
    request: &IssueRequest<'_>,
) -> Result<IssuedCertificate, String> {
    let primary = request
        .names
        .first()
        .ok_or_else(|| "证书至少需要一个域名".to_string())?;
    tokio::fs::create_dir_all(request.storage_dir)
        .await
        .map_err(|error| format!("创建证书目录失败：{error}"))?;
    let account = account(request).await?;
    let identifiers: Vec<Identifier> = request
        .names
        .iter()
        .map(|name| Identifier::Dns(name.clone()))
        .collect();
    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .map_err(|error| format!("创建证书订单失败：{error}"))?;

    let mut created_records = Vec::new();
    let result = async {
        answer_challenges(
            &mut order,
            cloudflare,
            request.propagation_timeout,
            &mut created_records,
        )
        .await?;
        finalize_order(&mut order).await
    }
    .await;

    for (zone_id, record_id) in &created_records {
        if let Err(error) = cloudflare.delete_record(zone_id, record_id).await {
            tracing::warn!(error, "failed to remove ACME challenge TXT record");
        }
    }
    let (private_key, chain) = result?;

    let folder = request.storage_dir.join(storage_name(primary));
    tokio::fs::create_dir_all(&folder)
        .await
        .map_err(|error| format!("创建证书目录失败：{error}"))?;
    let cert_path = folder.join("fullchain.pem");
    let key_path = folder.join("privkey.pem");
    write_private_file(&key_path, private_key.as_bytes()).await?;
    tokio::fs::write(&cert_path, chain.as_bytes())
        .await
        .map_err(|error| format!("保存证书失败：{error}"))?;
    let info = inspect_pem(chain.as_bytes())?;
    Ok(IssuedCertificate {
        cert_path,
        key_path,
        not_before: info.not_before,
        not_after: info.not_after,
        issuer: info.issuer,
    })
}

/// Publishes a TXT record for every pending authorization, waits for DNS and
/// tells the CA to validate. Created records are pushed to `created` for cleanup.
async fn answer_challenges(
    order: &mut Order,
    cloudflare: &CloudflareApi,
    propagation_timeout: Duration,
    created: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let mut pending = Vec::new();
    let mut authorizations = order.authorizations();
    while let Some(authorization) = authorizations.next().await {
        let mut authorization =
            authorization.map_err(|error| format!("读取域名授权失败：{error}"))?;
        match authorization.status {
            AuthorizationStatus::Pending => {}
            AuthorizationStatus::Valid => continue,
            status => return Err(format!("域名授权状态异常：{status:?}")),
        }
        let challenge = authorization
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| "证书颁发机构没有提供 DNS-01 验证方式".to_string())?;
        let Identifier::Dns(name) = challenge.identifier().identifier else {
            return Err("只支持域名类型的证书".to_string());
        };
        let record_name = challenge_record_name(name);
        let value = challenge.key_authorization().dns_value();
        created.push(cloudflare.create_txt(&record_name, &value).await?);
        pending.push((record_name, value));
    }
    for (record_name, value) in &pending {
        wait_for_txt(record_name, value, propagation_timeout).await;
    }

    let mut authorizations = order.authorizations();
    while let Some(authorization) = authorizations.next().await {
        let mut authorization =
            authorization.map_err(|error| format!("读取域名授权失败：{error}"))?;
        if authorization.status != AuthorizationStatus::Pending {
            continue;
        }
        if let Some(mut challenge) = authorization.challenge(ChallengeType::Dns01) {
            challenge
                .set_ready()
                .await
                .map_err(|error| format!("提交 DNS 验证失败：{error}"))?;
        }
    }
    Ok(())
}

/// Waits for validation, then returns `(private_key_pem, certificate_chain_pem)`.
async fn finalize_order(order: &mut Order) -> Result<(String, String), String> {
    let status = order
        .poll_ready(&RetryPolicy::new().timeout(Duration::from_mins(3)))
        .await
        .map_err(|error| format!("等待域名验证失败：{error}"))?;
    if status != OrderStatus::Ready {
        return Err(format!(
            "域名验证未通过（订单状态 {status:?}），请确认 Cloudflare Token 有 DNS 编辑权限且域名托管在该账号下"
        ));
    }
    let private_key = order
        .finalize()
        .await
        .map_err(|error| format!("提交证书签名请求失败：{error}"))?;
    let chain = order
        .poll_certificate(&RetryPolicy::new().timeout(Duration::from_mins(2)))
        .await
        .map_err(|error| format!("下载证书失败：{error}"))?;
    Ok((private_key, chain))
}

/// Validity and issuer of the leaf certificate in a PEM chain.
pub struct CertificateInfo {
    pub not_before: DateTime<FixedOffset>,
    pub not_after: DateTime<FixedOffset>,
    pub issuer: String,
    pub names: Vec<String>,
}

pub fn inspect_pem(pem: &[u8]) -> Result<CertificateInfo, String> {
    use x509_parser::prelude::*;

    let (_, pem) = parse_x509_pem(pem).map_err(|error| format!("证书格式无效：{error}"))?;
    let certificate = pem
        .parse_x509()
        .map_err(|error| format!("证书内容无效：{error}"))?;
    let timestamp = |time: ASN1Time| {
        Utc.timestamp_opt(time.timestamp(), 0)
            .single()
            .map(|time| time.fixed_offset())
            .ok_or_else(|| "证书有效期无效".to_string())
    };
    let names = certificate
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(name) => Some((*name).to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let issuer = certificate
        .issuer()
        .iter_organization()
        .chain(certificate.issuer().iter_common_name())
        .filter_map(|attribute| attribute.as_str().ok())
        .collect::<Vec<_>>()
        .join(" / ");
    Ok(CertificateInfo {
        not_before: timestamp(certificate.validity().not_before)?,
        not_after: timestamp(certificate.validity().not_after)?,
        issuer,
        names,
    })
}

/// Polls public DNS (DNS over HTTPS) until the TXT value is visible, giving up
/// silently after `timeout` — the CA performs its own lookup anyway.
async fn wait_for_txt(name: &str, value: &str, timeout: Duration) {
    if timeout.is_zero() {
        return;
    }
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return;
    };
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let visible = client
            .get("https://cloudflare-dns.com/dns-query")
            .query(&[("name", name), ("type", "TXT")])
            .header("accept", "application/dns-json")
            .send()
            .await
            .ok()
            .map(reqwest::Response::json::<serde_json::Value>);
        if let Some(response) = visible {
            if let Ok(body) = response.await {
                let found = body["Answer"].as_array().is_some_and(|answers| {
                    answers
                        .iter()
                        .filter_map(|answer| answer["data"].as_str())
                        .any(|data| data.trim_matches('"') == value)
                });
                if found {
                    // Give the remaining authoritative servers a moment too.
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    tokio::fs::write(path, contents)
        .await
        .map_err(|error| format!("保存 {} 失败：{error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_directories_are_normalized() {
        assert_eq!(storage_name("*.Example.com"), "_wildcard.example.com");
        assert_eq!(storage_name("a/b c.com"), "a_b_c.com");
        assert_eq!(
            challenge_record_name("*.example.com"),
            "_acme-challenge.example.com"
        );
        assert_eq!(
            challenge_record_name("app.example.com"),
            "_acme-challenge.app.example.com"
        );
        assert!(directory_url("staging").contains("staging"));
        assert!(!directory_url("").contains("staging"));
        assert_eq!(
            directory_url("https://acme.test/dir"),
            "https://acme.test/dir"
        );
    }

    #[test]
    fn certificate_metadata_is_read_from_pem() {
        let mut params =
            rcgen::CertificateParams::new(vec!["app.example.com".to_string()]).unwrap();
        params.not_after = rcgen::date_time_ymd(2031, 1, 2);
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        let info = inspect_pem(certificate.pem().as_bytes()).unwrap();
        assert_eq!(info.names, vec!["app.example.com"]);
        assert_eq!(info.not_after.format("%Y-%m-%d").to_string(), "2031-01-02");
        assert!(inspect_pem(b"not a certificate").is_err());
    }
}
