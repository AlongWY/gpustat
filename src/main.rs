extern crate nvml_wrapper as nvml;

use chrono::prelude::*;
use clap::Clap;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
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
#[clap(version = "0.1.4", author = "Feng Yunlong <ylfeng@ir.hit.edu.cn>")]
struct Opts {
    #[clap(long, about = "Force colored output (even when stdout is not a tty)")]
    color: bool,
    #[clap(long, about = "Suppress colored output")]
    no_color: bool,
    // #[clap(short = 'u', long, about = "Display username of the process owner")]
    // show_user: bool,
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
    #[clap(
        short = 'e',
        long,
        about = "Display encoder and/or decoder utilization"
    )]
    show_codec: bool,
    #[clap(short = 'a', long, about = "Display all gpu properties above")]
    show_all: bool,
}

macro_rules! bold_limit {
    ($value:ident, $limit:expr, $color:expr, $($arg:tt)*) => {{
        let cell = Cell::new(format!($($arg)*)).fg($color);
        if $value > $limit {
            cell.add_attribute(Attribute::Bold)
        } else {
            cell
        }
    }};
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
            let process = system.process(device_process.pid as i32).unwrap();
            let user = User::from_uid(Uid::from_raw(process.uid)).unwrap().unwrap();
            let used = match device_process.used_gpu_memory {
                UsedGpuMemory::Unavailable => String::from("Unavailable"),
                UsedGpuMemory::Used(m) => {
                    format!("{}M", m >> 20)
                }
            };

            let info = {
                let mut s = String::from(user.name);
                if opts.show_full_cmd || opts.show_all {
                    s = s + ":" + &process.cmd().join(" ");
                } else if opts.show_cmd {
                    s = s + ":" + process.name();
                }
                if opts.show_pid || opts.show_all {
                    s = s + "/" + &device_process.pid.to_string();
                }
                s
            };
            process_info.push(format!("{}({})", info, used));
        }

        let temperature = device.temperature(TemperatureSensor::Gpu)?; // 50
        let util_rates = device.utilization_rates()?.gpu; // 30

        let device_memory_rates = device_memory.used as f64 / device_memory.total as f64; // 50

        let temperature_cell = bold_limit!(temperature, 50, Color::Red, "{}°C", temperature);
        let utilization_cell = bold_limit!(util_rates, 30, Color::Green, "{} %", util_rates);

        let mut row = vec![
            Cell::new(format!("[{}]", index)).fg(Color::DarkCyan), // index
            Cell::new(device_name).fg(Color::DarkBlue),            // gpu type name
            temperature_cell,
            utilization_cell,
        ];

        if opts.show_fan || opts.show_all {
            let fan_color = Color::Rgb {
                r: 255,
                g: 0,
                b: 255,
            };
            let fan_rates = device.fan_speed(0)?; // 50
            let fan_cell = bold_limit!(fan_rates, 50, fan_color, "F: {} %", fan_rates);
            row.push(fan_cell);
        }

        if opts.show_codec || opts.show_all {
            let en_util_rates = device.encoder_utilization()?.utilization; // 30
            let de_util_rates = device.decoder_utilization()?.utilization; // 30

            let encoder_cell =
                bold_limit!(en_util_rates, 30, Color::Cyan, "E: {} %", en_util_rates);
            let decoder_cell =
                bold_limit!(de_util_rates, 30, Color::Cyan, "D: {} %", de_util_rates);

            row.push(encoder_cell);
            row.push(decoder_cell);
        }

        let pow_usage = device.power_usage()?;
        let pow_limit = device.power_management_limit()?;
        let pow_rates = pow_usage as f32 / pow_limit as f32; // 50
        let pow_cell = bold_limit!(
            pow_rates,
            0.5,
            Color::DarkMagenta,
            "{} / {} W",
            pow_usage / 1000,
            pow_limit / 1000
        );
        let memory_cell = bold_limit!(
            device_memory_rates,
            0.5,
            Color::Yellow,
            "{} / {} MB",
            device_memory.used >> 20,
            device_memory.total >> 20
        );

        row.push(pow_cell);
        row.push(memory_cell);
        row.push(Cell::new(process_info.join(",")).fg(Color::DarkYellow));

        table.add_row(row);
    }

    println!(
        "{}\t{}\t{}",
        hostname::get()?.to_str().unwrap_or_default(),
        localtime.format("%Y-%m-%d %H:%M:%S").to_string(),
        nvml.sys_driver_version()?
    );
    println!("{}", table);

    Ok(())
}
