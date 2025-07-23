use std::time::SystemTime;

pub fn now() -> SystemTime {
    #[cfg(not(target_family = "windows"))]
    return SystemTime::now();
    #[cfg(target_family = "windows")]
    #[allow(dead_code)]
    {
        debug_assert_eq!(size_of::<SystemTime>(), size_of::<SystemTime0>());
        
        use winapi::shared::minwindef::FILETIME;
        use winapi::um::sysinfoapi::GetSystemTimeAsFileTime;

        struct SystemTime0(SystemTime1);
        struct SystemTime1 {
            t: FILETIME,
        }
        
        let mut time: FILETIME = unsafe { std::mem::zeroed() };
        unsafe {
            GetSystemTimeAsFileTime(&mut time);
        }

        return unsafe {
            std::mem::transmute(SystemTime0(SystemTime1 { t: time }))
        };
    }
}