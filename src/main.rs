extern crate nvml_wrapper as nvml;

use chrono::prelude::*;
use clap::Clap;
use comfy_table::{Cell, Color, ContentArrangement, Table, Attribute};
use nix::unistd::{Uid, User};
use nvml::enum_wrappers::device::TemperatureSensor;
use nvml::enums::device::UsedGpuMemory;
use nvml::error::NvmlError;
use nvml::NVML;
use sysinfo::{ProcessExt, RefreshKind, System, SystemExt};

use thiserror::Error;

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum StatusError {
    #[error("Failed to parse hostname: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to convert string: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("Failed to load nvml library: {0}")]
    NvmlError(#[from] NvmlError),
}

#[derive(Clap)]
#[clap(version = "1.0", author = "Feng Yunlong <ylfeng@ir.hit.edu.cn>")]
struct Opts {
    #[clap(long, about = "Force colored output (even when stdout is not a tty)")]
    color: bool,
    #[clap(long, about = "Suppress colored output")]
    no_color: bool,
    #[clap(short = 'u', long, about = "Display username of the process owner")]
    show_user: bool,
    #[clap(short = 'c', long, about = "Display the process name")]
    show_cmd: bool,
    #[clap(
    short = 'f',
    long,
    about = "Display full command and cpu stats of running process"
    )]
    show_full_cmd: bool,
    #[clap(short = 'p', long, about = "Display PID of the process")]
    show_pid: bool,
    #[clap(short = 'F', long, about = "Display GPU fan speed")]
    show_fan: bool,
}

fn main() -> Result<(), StatusError> {
    let opts: Opts = Opts::parse();
    let localtime: DateTime<Local> = Local::now();

    let mut table = Table::new();

    table
        .load_preset("     ═  |          ")
        .set_content_arrangement(ContentArrangement::Dynamic);

    if opts.no_color {
        table.force_no_tty();
    } else if opts.color {
        table.enforce_styling();
    }

    let nvml = NVML::init()?;
    let device_num = nvml.device_count()?;

    let system = System::new_with_specifics(RefreshKind::new().with_processes());

    for index in 0..device_num {
        let device = nvml.device_by_index(index)?;
        let device_name = device.name()?;
        let device_memory = device.memory_info()?;
        let device_processes = device.running_compute_processes()?;

        let mut process_info = vec![];
        for device_process in device_processes {
            let process = system.get_process(device_process.pid as i32).unwrap();
            let user = User::from_uid(Uid::from_raw(process.uid)).unwrap().unwrap();
            let used = match device_process.used_gpu_memory {
                UsedGpuMemory::Unavailable => String::from("Unavailable"),
                UsedGpuMemory::Used(m) => {
                    format!("{}M", m >> 20)
                }
            };

            let info = {
                let mut s = String::from(user.name);
                if opts.show_cmd {
                    s = s + ":" + process.name();
                } else if opts.show_full_cmd {
                    s = s + ":" + &process.cmd().join(" ");
                }
                if opts.show_pid {
                    s = s + "/" + &device_process.pid.to_string();
                }
                s
            };
            process_info.push(format!("{}({})", info, used));
        }

        let temperature = device.temperature(TemperatureSensor::Gpu)?; // 50
        let utilization_rates = device.utilization_rates()?.gpu; // 30

        let temperature_cell = Cell::new(format!("{}'C", temperature))
            .fg(Color::Red);
        let utilization_rates_cell = Cell::new(format!("{} %", utilization_rates))
            .fg(Color::Green);
        let memory_cell = Cell::new(format!(
            "{} / {} MB",
            device_memory.used >> 20,
            device_memory.total >> 20
        ))
            .fg(Color::Yellow);
        table.add_row(vec![
            Cell::new(format!("[{}]", index)).fg(Color::DarkCyan),
            Cell::new(device_name).fg(Color::DarkBlue),
            if temperature <= 50 { temperature_cell } else { temperature_cell.add_attribute(Attribute::Bold) },
            if utilization_rates <= 30 { utilization_rates_cell } else { utilization_rates_cell.add_attribute(Attribute::Bold) },
            if device_memory.used <= 50 { memory_cell } else { memory_cell.add_attribute(Attribute::Bold) },
            Cell::new(process_info.join(",")).fg(Color::DarkYellow),
        ]);
    }

    println!("{} {}", hostname::get()?.to_str().unwrap_or_default(), localtime.format("%Y-%m-%d %H:%M:%S").to_string());
    println!("{}", table);

    Ok(())
}
