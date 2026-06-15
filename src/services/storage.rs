#![allow(clippy::missing_errors_doc)]

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use object_store::{
    aws::AmazonS3Builder, path::Path as ObjectPath, MultipartUpload, ObjectStore, PutPayload,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::{storage_buckets, storage_profiles},
};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StorageObjectRecord {
    pub key: String,
    pub name: String,
    pub prefix: String,
    pub url: String,
    pub size_bytes: i64,
    pub updated_at: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StoragePrefixRecord {
    pub prefix: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StorageBrowserRecord {
    pub prefixes: Vec<StoragePrefixRecord>,
    pub objects: Vec<StorageObjectRecord>,
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub object_key: String,
    pub bucket: String,
    pub prefix: Option<String>,
    pub etag: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub size_bytes: i64,
    pub updated_at: Option<String>,
    pub etag: Option<String>,
    pub url: String,
}

pub async fn put_object(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: Option<&str>,
    filename: &str,
    bytes: Vec<u8>,
) -> ApiResult<StoredObject> {
    let normalized_prefix = normalize_prefix(prefix)?;
    let object_key = join_key(normalized_prefix.as_deref(), filename);
    match profile.provider.as_str() {
        "local" => put_local(bucket, &object_key, &bytes)?,
        "s3_compatible" => put_s3(profile, bucket, &object_key, bytes).await?,
        _ => return Err(ApiError::bad_request("unsupported storage provider")),
    }

    Ok(StoredObject {
        object_key: object_key.clone(),
        bucket: bucket.bucket.clone(),
        prefix: normalized_prefix,
        etag: None,
        url: public_url(profile, bucket, &object_key),
    })
}

pub async fn get_object(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<Vec<u8>> {
    let object_key = normalize_object_key(object_key)?;
    match profile.provider.as_str() {
        "local" => get_local(bucket, &object_key),
        "s3_compatible" => get_s3(profile, bucket, &object_key).await,
        _ => Err(ApiError::bad_request("unsupported storage provider")),
    }
}

pub async fn put_object_from_parts(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
    parts: Vec<Vec<u8>>,
) -> ApiResult<StoredObject> {
    let object_key = normalize_object_key(object_key)?;
    match profile.provider.as_str() {
        "local" => put_local_parts(bucket, &object_key, &parts)?,
        "s3_compatible" => put_s3_parts(profile, bucket, &object_key, parts).await?,
        _ => return Err(ApiError::bad_request("unsupported storage provider")),
    }

    Ok(StoredObject {
        object_key: object_key.clone(),
        bucket: bucket.bucket.clone(),
        prefix: object_key
            .rsplit_once('/')
            .map(|(prefix, _)| format!("{prefix}/")),
        etag: None,
        url: public_url(profile, bucket, &object_key),
    })
}

pub async fn put_object_from_files(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
    part_paths: &[PathBuf],
) -> ApiResult<StoredObject> {
    let object_key = normalize_object_key(object_key)?;
    match profile.provider.as_str() {
        "local" => put_local_files(bucket, &object_key, part_paths)?,
        "s3_compatible" => put_s3_files(profile, bucket, &object_key, part_paths).await?,
        _ => return Err(ApiError::bad_request("unsupported storage provider")),
    }

    Ok(StoredObject {
        object_key: object_key.clone(),
        bucket: bucket.bucket.clone(),
        prefix: object_key
            .rsplit_once('/')
            .map(|(prefix, _)| format!("{prefix}/")),
        etag: None,
        url: public_url(profile, bucket, &object_key),
    })
}

pub async fn object_metadata(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<ObjectMetadata> {
    let object_key = normalize_object_key(object_key)?;
    match profile.provider.as_str() {
        "local" => metadata_local(profile, bucket, &object_key),
        "s3_compatible" => metadata_s3(profile, bucket, &object_key).await,
        _ => Err(ApiError::bad_request("unsupported storage provider")),
    }
}

pub async fn rename_object(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    source_key: &str,
    target_key: &str,
) -> ApiResult<StoredObject> {
    let source_key = normalize_object_key(source_key)?;
    let target_key = normalize_object_key(target_key)?;
    if source_key == target_key {
        return Ok(StoredObject {
            object_key: target_key.clone(),
            bucket: bucket.bucket.clone(),
            prefix: target_key
                .rsplit_once('/')
                .map(|(prefix, _)| format!("{prefix}/")),
            etag: object_metadata(profile, bucket, &target_key)
                .await
                .ok()
                .and_then(|metadata| metadata.etag),
            url: public_url(profile, bucket, &target_key),
        });
    }
    match profile.provider.as_str() {
        "local" => rename_local(bucket, &source_key, &target_key)?,
        "s3_compatible" => rename_s3(profile, bucket, &source_key, &target_key).await?,
        _ => return Err(ApiError::bad_request("unsupported storage provider")),
    }
    let metadata = object_metadata(profile, bucket, &target_key).await.ok();
    Ok(StoredObject {
        object_key: target_key.clone(),
        bucket: bucket.bucket.clone(),
        prefix: target_key
            .rsplit_once('/')
            .map(|(prefix, _)| format!("{prefix}/")),
        etag: metadata.and_then(|metadata| metadata.etag),
        url: public_url(profile, bucket, &target_key),
    })
}

pub async fn delete_object(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<()> {
    let object_key = normalize_object_key(object_key)?;
    match profile.provider.as_str() {
        "local" => delete_local(bucket, &object_key),
        "s3_compatible" => delete_s3(profile, bucket, &object_key).await,
        _ => Err(ApiError::bad_request("unsupported storage provider")),
    }
}

pub async fn create_folder(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: &str,
) -> ApiResult<StoragePrefixRecord> {
    let prefix = normalize_prefix(Some(prefix))?
        .ok_or_else(|| ApiError::bad_request("folder prefix is required"))?;
    match profile.provider.as_str() {
        "local" => create_local_folder(bucket, &prefix)?,
        "s3_compatible" => create_s3_folder(profile, bucket, &prefix).await?,
        _ => return Err(ApiError::bad_request("unsupported storage provider")),
    }
    Ok(StoragePrefixRecord {
        name: prefix
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(prefix.as_str())
            .to_string(),
        prefix,
    })
}

pub async fn list_objects(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: Option<&str>,
) -> ApiResult<StorageBrowserRecord> {
    let normalized_prefix = normalize_prefix(prefix)?;
    match profile.provider.as_str() {
        "local" => list_local(profile, bucket, normalized_prefix.as_deref()),
        "s3_compatible" => list_s3(profile, bucket, normalized_prefix.as_deref()).await,
        _ => Err(ApiError::bad_request("unsupported storage provider")),
    }
}

pub fn ensure_provider_supported(provider: &str) -> ApiResult<()> {
    match provider {
        "local" | "s3_compatible" => Ok(()),
        _ => Err(ApiError::bad_request("unsupported storage provider")),
    }
}

pub fn normalize_prefix(prefix: Option<&str>) -> ApiResult<Option<String>> {
    let Some(prefix) = prefix.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = normalize_object_key(prefix)?;
    Ok(Some(format!("{}/", normalized.trim_end_matches('/'))))
}

pub fn normalize_object_key(key: &str) -> ApiResult<String> {
    let key = key.trim().trim_start_matches('/');
    if key.is_empty() {
        return Err(ApiError::bad_request("object key is required"));
    }
    if key
        .split('/')
        .any(|segment| segment == ".." || segment == ".")
    {
        return Err(ApiError::bad_request("invalid object key"));
    }
    Ok(key.to_string())
}

fn put_local(bucket: &storage_buckets::Model, object_key: &str, bytes: &[u8]) -> ApiResult<()> {
    let path = local_path(bucket, object_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    }
    fs::write(path, bytes).map_err(|_| ApiError::internal("failed to save uploaded file"))
}

fn put_local_parts(
    bucket: &storage_buckets::Model,
    object_key: &str,
    parts: &[Vec<u8>],
) -> ApiResult<()> {
    let path = local_path(bucket, object_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    }
    let mut file = fs::File::create(path)
        .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    for part in parts {
        std::io::Write::write_all(&mut file, part)
            .map_err(|_| ApiError::internal("failed to save uploaded file"))?;
    }
    Ok(())
}

fn put_local_files(
    bucket: &storage_buckets::Model,
    object_key: &str,
    part_paths: &[PathBuf],
) -> ApiResult<()> {
    let path = local_path(bucket, object_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    }
    let mut file = fs::File::create(path)
        .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    for part_path in part_paths {
        let mut part = fs::File::open(part_path)
            .map_err(|_| ApiError::bad_request("upload task chunk is missing"))?;
        std::io::copy(&mut part, &mut file)
            .map_err(|_| ApiError::internal("failed to save uploaded file"))?;
    }
    Ok(())
}

fn get_local(bucket: &storage_buckets::Model, object_key: &str) -> ApiResult<Vec<u8>> {
    fs::read(local_path(bucket, object_key)?)
        .map_err(|_| ApiError::bad_request("file content not found"))
}

fn rename_local(
    bucket: &storage_buckets::Model,
    source_key: &str,
    target_key: &str,
) -> ApiResult<()> {
    let source = local_path(bucket, source_key)?;
    let target = local_path(bucket, target_key)?;
    if target.exists() {
        return Err(ApiError::bad_request("target object already exists"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare upload storage"))?;
    }
    fs::rename(source, target).map_err(|_| ApiError::internal("failed to rename stored object"))
}

fn delete_local(bucket: &storage_buckets::Model, object_key: &str) -> ApiResult<()> {
    match fs::remove_file(local_path(bucket, object_key)?) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ApiError::internal("failed to delete stored object")),
    }
}

fn create_local_folder(bucket: &storage_buckets::Model, prefix: &str) -> ApiResult<()> {
    fs::create_dir_all(local_path(bucket, prefix)?)
        .map_err(|_| ApiError::internal("failed to create storage folder"))
}

fn metadata_local(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<ObjectMetadata> {
    let metadata = fs::metadata(local_path(bucket, object_key)?)
        .map_err(|_| ApiError::bad_request("file content not found"))?;
    if !metadata.is_file() {
        return Err(ApiError::bad_request("object is not a file"));
    }
    Ok(ObjectMetadata {
        size_bytes: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        updated_at: metadata
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|time| time.to_rfc3339()),
        etag: None,
        url: public_url(profile, bucket, object_key),
    })
}

fn list_local(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: Option<&str>,
) -> ApiResult<StorageBrowserRecord> {
    let root = local_root(bucket)?;
    let dir = match prefix {
        Some(prefix) => root.join(prefix),
        None => root,
    };
    let mut prefixes = Vec::new();
    let mut objects = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StorageBrowserRecord { prefixes, objects });
        }
        Err(_) => return Err(ApiError::internal("failed to list local storage")),
    };

    for entry in entries {
        let entry = entry.map_err(|_| ApiError::internal("failed to list local storage"))?;
        let metadata = entry
            .metadata()
            .map_err(|_| ApiError::internal("failed to read local file metadata"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let key = join_key(prefix, &name);
        if metadata.is_dir() {
            prefixes.push(StoragePrefixRecord {
                prefix: format!("{}/", key.trim_end_matches('/')),
                name,
            });
        } else if metadata.is_file() {
            objects.push(StorageObjectRecord {
                key: key.clone(),
                name,
                prefix: prefix.unwrap_or_default().to_string(),
                url: public_url(profile, bucket, &key),
                size_bytes: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
                updated_at: metadata
                    .modified()
                    .ok()
                    .map(chrono::DateTime::<chrono::Utc>::from)
                    .map(|time| time.to_rfc3339()),
                etag: None,
            });
        }
    }
    prefixes.sort_by(|left, right| left.prefix.cmp(&right.prefix));
    objects.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(StorageBrowserRecord { prefixes, objects })
}

async fn put_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
    bytes: Vec<u8>,
) -> ApiResult<()> {
    let store = s3_store(profile, bucket)?;
    store
        .put(&ObjectPath::from(object_key), PutPayload::from(bytes))
        .await
        .map_err(|_| ApiError::internal("failed to save object to s3-compatible storage"))?;
    Ok(())
}

async fn put_s3_parts(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
    parts: Vec<Vec<u8>>,
) -> ApiResult<()> {
    let store = s3_store(profile, bucket)?;
    let mut upload = store
        .put_multipart(&ObjectPath::from(object_key))
        .await
        .map_err(|_| ApiError::internal("failed to start multipart upload"))?;
    for part in parts {
        upload
            .put_part(PutPayload::from(part))
            .await
            .map_err(|_| ApiError::internal("failed to save multipart upload chunk"))?;
    }
    upload
        .complete()
        .await
        .map_err(|_| ApiError::internal("failed to complete multipart upload"))?;
    Ok(())
}

async fn put_s3_files(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
    part_paths: &[PathBuf],
) -> ApiResult<()> {
    let store = s3_store(profile, bucket)?;
    let mut upload = store
        .put_multipart(&ObjectPath::from(object_key))
        .await
        .map_err(|_| ApiError::internal("failed to start multipart upload"))?;
    for part_path in part_paths {
        let part = fs::read(part_path)
            .map_err(|_| ApiError::bad_request("upload task chunk is missing"))?;
        upload
            .put_part(PutPayload::from(part))
            .await
            .map_err(|_| ApiError::internal("failed to save multipart upload chunk"))?;
    }
    upload
        .complete()
        .await
        .map_err(|_| ApiError::internal("failed to complete multipart upload"))?;
    Ok(())
}

async fn get_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<Vec<u8>> {
    let store = s3_store(profile, bucket)?;
    let bytes = store
        .get(&ObjectPath::from(object_key))
        .await
        .map_err(|_| ApiError::bad_request("file content not found"))?
        .bytes()
        .await
        .map_err(|_| ApiError::internal("failed to read object content"))?;
    Ok(bytes.to_vec())
}

async fn rename_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    source_key: &str,
    target_key: &str,
) -> ApiResult<()> {
    let bytes = get_s3(profile, bucket, source_key).await?;
    put_s3(profile, bucket, target_key, bytes).await?;
    delete_s3(profile, bucket, source_key).await
}

async fn delete_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<()> {
    let store = s3_store(profile, bucket)?;
    store
        .delete(&ObjectPath::from(object_key))
        .await
        .map_err(|_| ApiError::internal("failed to delete s3-compatible object"))
}

async fn create_s3_folder(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: &str,
) -> ApiResult<()> {
    put_s3(profile, bucket, &format!("{prefix}.keep"), Vec::new()).await
}

async fn metadata_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> ApiResult<ObjectMetadata> {
    let store = s3_store(profile, bucket)?;
    let metadata = store
        .head(&ObjectPath::from(object_key))
        .await
        .map_err(|_| ApiError::bad_request("file content not found"))?;
    Ok(ObjectMetadata {
        size_bytes: i64::try_from(metadata.size).unwrap_or(i64::MAX),
        updated_at: Some(metadata.last_modified.to_rfc3339()),
        etag: metadata.e_tag,
        url: public_url(profile, bucket, object_key),
    })
}

async fn list_s3(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    prefix: Option<&str>,
) -> ApiResult<StorageBrowserRecord> {
    let store = s3_store(profile, bucket)?;
    let prefix_path = ObjectPath::from(prefix.unwrap_or_default());
    let result = store
        .list_with_delimiter(Some(&prefix_path))
        .await
        .map_err(|_| ApiError::internal("failed to list s3-compatible storage"))?;
    let prefixes = result
        .common_prefixes
        .into_iter()
        .map(|prefix| {
            let prefix = prefix.to_string();
            StoragePrefixRecord {
                name: prefix
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(prefix.as_str())
                    .to_string(),
                prefix,
            }
        })
        .collect();
    let objects = result
        .objects
        .into_iter()
        .map(|object| {
            let key = object.location.to_string();
            StorageObjectRecord {
                name: key.rsplit('/').next().unwrap_or(key.as_str()).to_string(),
                prefix: prefix.unwrap_or_default().to_string(),
                url: public_url(profile, bucket, &key),
                key,
                size_bytes: i64::try_from(object.size).unwrap_or(i64::MAX),
                updated_at: Some(object.last_modified.to_rfc3339()),
                etag: object.e_tag,
            }
        })
        .collect();
    Ok(StorageBrowserRecord { prefixes, objects })
}

fn s3_store(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
) -> ApiResult<object_store::aws::AmazonS3> {
    let endpoint = profile
        .endpoint
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("storage endpoint is required"))?;
    let access_key_id = profile
        .access_key_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("access key id is required"))?;
    let secret_access_key = profile
        .secret_access_key
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("secret access key is required"))?;
    let region = profile.region.as_deref().unwrap_or("auto");

    AmazonS3Builder::new()
        .with_endpoint(endpoint)
        .with_bucket_name(&bucket.bucket)
        .with_region(region)
        .with_access_key_id(access_key_id)
        .with_secret_access_key(secret_access_key)
        .with_allow_http(endpoint.starts_with("http://"))
        .build()
        .map_err(|_| ApiError::bad_request("invalid s3-compatible storage configuration"))
}

fn local_root(bucket: &storage_buckets::Model) -> ApiResult<PathBuf> {
    let root = bucket
        .local_root
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("local root is required"))?;
    Ok(PathBuf::from(root))
}

fn local_path(bucket: &storage_buckets::Model, object_key: &str) -> ApiResult<PathBuf> {
    let root = local_root(bucket)?;
    let path = Path::new(object_key);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ApiError::bad_request("invalid object key"));
    }
    Ok(root.join(path))
}

fn join_key(prefix: Option<&str>, filename: &str) -> String {
    prefix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || filename.to_string(),
            |prefix| format!("{}/{filename}", prefix.trim_end_matches('/')),
        )
}

fn public_url(
    profile: &storage_profiles::Model,
    bucket: &storage_buckets::Model,
    object_key: &str,
) -> String {
    let Some(base) = profile
        .public_base_url
        .as_deref()
        .or(bucket.public_prefix.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return String::new();
    };
    format!("{}/{}", base.trim_end_matches('/'), object_key)
}
