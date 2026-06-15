#![allow(clippy::missing_errors_doc)]

use std::process::Command;

use chrono::Utc;
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use sysinfo::{Disks, Networks, System};

use crate::{
    controllers::admin::authorize,
    errors::ApiResult,
    models::_entities::{database_backups, operation_logs, rate_limit_events, scheduled_task_runs},
    responses::{self, ApiResponse, PageResponse},
};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MonitoringOverview {
    pub db_ok: bool,
    pub task_success_count: u64,
    pub task_failed_count: u64,
    pub backup_success_count: u64,
    pub backup_failed_count: u64,
    pub rate_limit_event_count: u64,
    pub error_log_count: u64,
    pub health_links: Vec<String>,
    pub server: ServerMonitorSnapshot,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ServerMonitorSnapshot {
    pub captured_at: String,
    pub host: HostInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
    pub gpus: Vec<GpuInfo>,
    pub gpu_available: bool,
    pub gpu_message: Option<String>,
    pub load: Option<LoadInfo>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HostInfo {
    pub hostname: Option<String>,
    pub system_name: Option<String>,
    pub kernel_version: Option<String>,
    pub os_version: Option<String>,
    pub long_os_version: Option<String>,
    pub architecture: String,
    pub uptime_seconds: u64,
    pub boot_time_seconds: u64,
    pub process_count: usize,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CpuInfo {
    pub global_usage: f32,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
    pub brand: Option<String>,
    pub frequency_mhz: u64,
    pub cores: Vec<CpuCoreInfo>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CpuCoreInfo {
    pub name: String,
    pub usage: f32,
    pub frequency_mhz: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub used_percent: f32,
    pub swap_used_percent: f32,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f32,
    pub removable: bool,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct NetworkInfo {
    pub interface: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_packets: u64,
    pub transmitted_packets: u64,
    pub received_errors: u64,
    pub transmitted_errors: u64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct GpuInfo {
    pub name: String,
    pub driver_version: Option<String>,
    pub utilization_percent: Option<f32>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub temperature_celsius: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct LoadInfo {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ProcessQueryParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
}

impl ProcessQueryParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(20).clamp(10, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ProcessRecord {
    pub pid: String,
    pub name: String,
    pub exe: Option<String>,
    pub command: String,
    pub status: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub run_time_seconds: u64,
    pub start_time_seconds: u64,
    pub user_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/monitoring/overview",
    tag = "admin-monitoring",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<MonitoringOverview>))
)]
#[debug_handler]
pub async fn overview(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:monitor:view").await?;

    let db_ok = ctx.db.ping().await.is_ok();
    let task_success_count = scheduled_task_runs::Entity::find()
        .filter(scheduled_task_runs::Column::Status.eq("success"))
        .count(&ctx.db)
        .await?;
    let task_failed_count = scheduled_task_runs::Entity::find()
        .filter(scheduled_task_runs::Column::Status.eq("failed"))
        .count(&ctx.db)
        .await?;
    let backup_success_count = database_backups::Entity::find()
        .filter(database_backups::Column::Status.eq("success"))
        .count(&ctx.db)
        .await?;
    let backup_failed_count = database_backups::Entity::find()
        .filter(database_backups::Column::Status.eq("failed"))
        .count(&ctx.db)
        .await?;
    let rate_limit_event_count = rate_limit_events::Entity::find().count(&ctx.db).await?;
    let error_log_count = operation_logs::Entity::find()
        .filter(operation_logs::Column::Level.eq("error"))
        .count(&ctx.db)
        .await?;

    Ok(responses::ok(MonitoringOverview {
        db_ok,
        task_success_count,
        task_failed_count,
        backup_success_count,
        backup_failed_count,
        rate_limit_event_count,
        error_log_count,
        health_links: vec![
            "/_health".to_string(),
            "/_readiness".to_string(),
            "/_ping".to_string(),
        ],
        server: collect_server_snapshot(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/admin/monitoring/server",
    tag = "admin-monitoring",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<ServerMonitorSnapshot>))
)]
#[debug_handler]
pub async fn server(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:monitor:view").await?;
    Ok(responses::ok(collect_server_snapshot()))
}

#[utoipa::path(
    get,
    path = "/api/admin/monitoring/processes",
    tag = "admin-monitoring",
    security(("bearer_auth" = [])),
    params(ProcessQueryParams),
    responses((status = 200, body = ApiResponse<PageResponse<ProcessRecord>>))
)]
#[debug_handler]
pub async fn processes(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Query(params): Query<ProcessQueryParams>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:monitor:view").await?;
    let page = params.page();
    let page_size = params.page_size();
    let mut records = collect_processes(&params);
    let total = u64::try_from(records.len()).unwrap_or(u64::MAX);
    let start = usize::try_from((page - 1).saturating_mul(page_size)).unwrap_or(usize::MAX);
    let take = usize::try_from(page_size).unwrap_or(100);
    let items = if start >= records.len() {
        Vec::new()
    } else {
        records
            .drain(start..records.len().min(start + take))
            .collect()
    };

    Ok(responses::ok(PageResponse {
        items,
        page,
        page_size,
        total,
    }))
}

fn collect_server_snapshot() -> ServerMonitorSnapshot {
    let mut system = System::new_all();
    system.refresh_all();
    let disks = Disks::new_with_refreshed_list();
    let networks = Networks::new_with_refreshed_list();
    let gpus = collect_gpus();
    let gpu_available = !gpus.is_empty();
    let gpu_message = if gpu_available {
        None
    } else {
        Some("未检测到 NVIDIA GPU 或当前环境不支持 GPU 采集".to_string())
    };

    ServerMonitorSnapshot {
        captured_at: Utc::now().to_rfc3339(),
        host: HostInfo {
            hostname: System::host_name(),
            system_name: System::name(),
            kernel_version: System::kernel_version(),
            os_version: System::os_version(),
            long_os_version: System::long_os_version(),
            architecture: std::env::consts::ARCH.to_string(),
            uptime_seconds: System::uptime(),
            boot_time_seconds: System::boot_time(),
            process_count: system.processes().len(),
        },
        cpu: collect_cpu(&system),
        memory: collect_memory(&system),
        disks: collect_disks(&disks),
        networks: collect_networks(&networks),
        gpus,
        gpu_available,
        gpu_message,
        load: collect_load(),
    }
}

fn collect_cpu(system: &System) -> CpuInfo {
    let cores = system
        .cpus()
        .iter()
        .map(|cpu| CpuCoreInfo {
            name: cpu.name().to_string(),
            usage: cpu.cpu_usage(),
            frequency_mhz: cpu.frequency(),
        })
        .collect::<Vec<_>>();
    CpuInfo {
        global_usage: system.global_cpu_usage(),
        logical_cores: system.cpus().len(),
        physical_cores: system.physical_core_count(),
        brand: system.cpus().first().map(|cpu| cpu.brand().to_string()),
        frequency_mhz: system.cpus().first().map_or(0, sysinfo::Cpu::frequency),
        cores,
    }
}

fn collect_memory(system: &System) -> MemoryInfo {
    let total_bytes = system.total_memory();
    let used_bytes = system.used_memory();
    let swap_total_bytes = system.total_swap();
    let swap_used_bytes = system.used_swap();
    MemoryInfo {
        total_bytes,
        used_bytes,
        free_bytes: system.free_memory(),
        available_bytes: system.available_memory(),
        swap_total_bytes,
        swap_used_bytes,
        used_percent: percent(used_bytes, total_bytes),
        swap_used_percent: percent(swap_used_bytes, swap_total_bytes),
    }
}

fn collect_disks(disks: &Disks) -> Vec<DiskInfo> {
    disks
        .iter()
        .map(|disk| {
            let total_bytes = disk.total_space();
            let available_bytes = disk.available_space();
            let used_bytes = total_bytes.saturating_sub(available_bytes);
            DiskInfo {
                name: disk.name().to_string_lossy().into_owned(),
                mount_point: disk.mount_point().to_string_lossy().into_owned(),
                file_system: disk.file_system().to_string_lossy().into_owned(),
                total_bytes,
                available_bytes,
                used_bytes,
                used_percent: percent(used_bytes, total_bytes),
                removable: disk.is_removable(),
            }
        })
        .collect()
}

fn collect_networks(networks: &Networks) -> Vec<NetworkInfo> {
    networks
        .iter()
        .map(|(interface, data)| NetworkInfo {
            interface: interface.clone(),
            received_bytes: data.received(),
            transmitted_bytes: data.transmitted(),
            received_packets: data.packets_received(),
            transmitted_packets: data.packets_transmitted(),
            received_errors: data.errors_on_received(),
            transmitted_errors: data.errors_on_transmitted(),
        })
        .collect()
}

fn collect_load() -> Option<LoadInfo> {
    let load = System::load_average();
    if load.one == 0.0 && load.five == 0.0 && load.fifteen == 0.0 {
        return None;
    }
    Some(LoadInfo {
        one: load.one,
        five: load.five,
        fifteen: load.fifteen,
    })
}

fn collect_processes(params: &ProcessQueryParams) -> Vec<ProcessRecord> {
    let mut system = System::new_all();
    system.refresh_all();
    let keyword = params
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let mut records = system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let name = process.name().to_string_lossy().into_owned();
            let command = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            ProcessRecord {
                pid: pid.to_string(),
                name,
                exe: process
                    .exe()
                    .map(|path| path.to_string_lossy().into_owned()),
                command,
                status: process.status().to_string(),
                cpu_usage: process.cpu_usage(),
                memory_bytes: process.memory(),
                virtual_memory_bytes: process.virtual_memory(),
                run_time_seconds: process.run_time(),
                start_time_seconds: process.start_time(),
                user_id: process.user_id().map(|user_id| format!("{user_id:?}")),
            }
        })
        .filter(|record| {
            keyword.as_ref().is_none_or(|keyword| {
                record.name.to_lowercase().contains(keyword)
                    || record.command.to_lowercase().contains(keyword)
                    || record.pid.contains(keyword)
            })
        })
        .collect::<Vec<_>>();

    sort_processes(&mut records, params);
    records
}

fn sort_processes(records: &mut [ProcessRecord], params: &ProcessQueryParams) {
    let sort = params.sort.as_deref().unwrap_or("cpu");
    let desc = !matches!(params.order.as_deref(), Some("asc"));
    records.sort_by(|left, right| {
        let ordering = match sort {
            "memory" => left.memory_bytes.cmp(&right.memory_bytes),
            "virtual_memory" => left.virtual_memory_bytes.cmp(&right.virtual_memory_bytes),
            "pid" => left.pid.cmp(&right.pid),
            "name" => left.name.cmp(&right.name),
            "run_time" => left.run_time_seconds.cmp(&right.run_time_seconds),
            _ => left.cpu_usage.total_cmp(&right.cpu_usage),
        };
        if desc {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn collect_gpus() -> Vec<GpuInfo> {
    let Ok(output) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,utilization.gpu,memory.total,memory.used,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_gpu_line)
        .collect()
}

fn parse_gpu_line(line: &str) -> Option<GpuInfo> {
    let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() < 6 || parts[0].is_empty() {
        return None;
    }
    Some(GpuInfo {
        name: parts[0].to_string(),
        driver_version: optional_string(parts[1]),
        utilization_percent: parse_optional_f32(parts[2]),
        memory_total_bytes: parse_optional_mib(parts[3]),
        memory_used_bytes: parse_optional_mib(parts[4]),
        temperature_celsius: parse_optional_f32(parts[5]),
    })
}

fn optional_string(value: &str) -> Option<String> {
    if value.is_empty() || value.eq_ignore_ascii_case("[not supported]") {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_optional_f32(value: &str) -> Option<f32> {
    value.parse::<f32>().ok()
}

fn parse_optional_mib(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .map(|mib| mib.saturating_mul(1024 * 1024))
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let basis_points = used.saturating_mul(10_000).checked_div(total).unwrap_or(0);
    f32::from(u16::try_from(basis_points.min(10_000)).unwrap_or(10_000)) / 100.0
}
