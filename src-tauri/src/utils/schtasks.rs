use anyhow::{Result, anyhow};
use clash_verge_logging::{Type, logging};
use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, Output};
use winapi::um::stringapiset::MultiByteToWideChar;
use winapi::um::winnls::{GetACP, GetOEMCP};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const TASK_NAME_ADMIN: &str = "Clash Verge (Admin)";

fn get_exe_path() -> Result<PathBuf> {
    let exe_path = std::env::current_exe().map_err(|e| anyhow!("failed to get exe path: {}", e))?;
    Ok(exe_path)
}

fn build_task_command() -> Result<String> {
    let exe_path = get_exe_path()?;
    Ok(format!("\"{}\"", exe_path.to_string_lossy()))
}

fn decode_with_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }

    let len = bytes.len();
    if len > i32::MAX as usize {
        return None;
    }

    let required = unsafe {
        MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr() as *const i8,
            len as i32,
            std::ptr::null_mut(),
            0,
        )
    };

    if required == 0 {
        return None;
    }

    let mut wide = vec![0u16; required as usize];
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr() as *const i8,
            len as i32,
            wide.as_mut_ptr(),
            required,
        )
    };

    if written == 0 {
        return None;
    }

    wide.truncate(written as usize);
    Some(String::from_utf16_lossy(&wide))
}

fn decode_console_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let oem = unsafe { GetOEMCP() };
    if let Some(text) = decode_with_code_page(bytes, oem) {
        return text;
    }

    let acp = unsafe { GetACP() };
    if let Some(text) = decode_with_code_page(bytes, acp) {
        return text;
    }

    String::from_utf8_lossy(bytes).to_string()
}

fn output_message(output: &Output) -> String {
    let stdout = decode_console_output(&output.stdout);
    let stderr = decode_console_output(&output.stderr);
    let stdout = stdout.trim();
    let stderr = stderr.trim();

    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "unknown error".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout} | {stderr}"),
    }
}

fn schtasks_output(mut cmd: Command) -> Result<Output> {
    cmd.creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| anyhow!("failed to execute schtasks: {}", e))
}

pub fn is_task_enabled() -> Result<bool> {
    let output = schtasks_output({
        let mut cmd = Command::new("schtasks");
        cmd.args(["/Query", "/TN", TASK_NAME_ADMIN]);
        cmd
    })?;

    Ok(output.status.success())
}

pub fn create_task() -> Result<()> {
    let task_command = build_task_command()?;
    let output = schtasks_output({
        let mut cmd = Command::new("schtasks");
        cmd.args(["/Create", "/SC", "ONLOGON"]);
        cmd.arg("/TN").arg(TASK_NAME_ADMIN);
        cmd.arg("/TR").arg(task_command);
        cmd.arg("/RL").arg("HIGHEST");
        cmd.arg("/F");
        cmd
    })?;

    if !output.status.success() {
        return Err(anyhow!("failed to create admin task: {}", output_message(&output)));
    }

    logging!(info, Type::Setup, "Created admin auto-launch task");
    Ok(())
}

pub fn remove_task() -> Result<()> {
    let output = schtasks_output({
        let mut cmd = Command::new("schtasks");
        cmd.args(["/Delete", "/TN", TASK_NAME_ADMIN, "/F"]);
        cmd
    })?;

    if output.status.success() {
        logging!(info, Type::Setup, "Removed admin auto-launch task");
        return Ok(());
    }

    if !is_task_enabled()? {
        logging!(info, Type::Setup, "Admin auto-launch task not found, skipping removal");
        return Ok(());
    }

    Err(anyhow!("failed to remove admin task: {}", output_message(&output)))
}

pub fn set_auto_launch(is_enable: bool) -> Result<()> {
    if is_enable {
        create_task()?;
    } else {
        remove_task()?;
    }

    Ok(())
}
