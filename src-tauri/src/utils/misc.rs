#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::CloseHandle;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevationType, TokenElevationTypeFull, TOKEN_ELEVATION_TYPE,
    TOKEN_QUERY,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub fn is_elevated() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            let process = GetCurrentProcess();
            let mut token = std::ptr::null_mut();

            // BOOL == i32, success is nonzero
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0 {
                let mut elevation_type: TOKEN_ELEVATION_TYPE = 0;
                let mut return_length: u32 = 0;

                if GetTokenInformation(
                    token,
                    TokenElevationType,
                    &mut elevation_type as *mut TOKEN_ELEVATION_TYPE as *mut _,
                    std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                    &mut return_length,
                ) != 0
                {
                    CloseHandle(token);
                    return Ok(elevation_type == TokenElevationTypeFull);
                }

                CloseHandle(token);
            }
        }
    }

    // On Linux/macOS, symlinks don't require elevation — always return true.
    // The original geteuid() == 0 check was incorrect for this use case:
    // this function gates symlink creation, which only needs admin on Windows.
    #[cfg(not(target_os = "windows"))]
    return Ok(true);

    #[cfg(target_os = "windows")]
    Ok(false)
}
