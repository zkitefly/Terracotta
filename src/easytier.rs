use crate::EASYTIER_DIR;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::{
    env, fs,
    io::{BufRead, BufReader, Cursor, Error},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use std::str::FromStr;

static EASYTIER_ARCHIVE: (&str, &str, &[u8]) = (
    include_str!(env!("TERRACOTTA_ET_ENTRY_CONF")),
    include_str!(env!("TERRACOTTA_ET_CLI_CONF")),
    include_bytes!(env!("TERRACOTTA_ET_ARCHIVE")),
);

lazy_static::lazy_static! {
    pub static ref FACTORY: EasytierFactory = create();
}

pub struct EasytierFactory {
    exe: PathBuf,
    cli: PathBuf,
}

pub struct Easytier {
    process: Arc<Mutex<Child>>,
    rpc: u16,
    cli: PathBuf,
}

fn create() -> EasytierFactory {
    let _ = fs::create_dir_all(&*EASYTIER_DIR);

    logging!(
        "Easytier",
        "Releasing easytier to {}",
        &*EASYTIER_DIR.to_string_lossy()
    );

    sevenz_rust2::decompress(Cursor::new(EASYTIER_ARCHIVE.2.to_vec()), &*EASYTIER_DIR)
        .map_err(|e| Error::other(e.to_string()))
        .unwrap();

    let exe: PathBuf = Path::join(&EASYTIER_DIR, EASYTIER_ARCHIVE.0);
    let cli: PathBuf = Path::join(&EASYTIER_DIR, EASYTIER_ARCHIVE.1);
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(exe.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(exe.clone(), permissions).unwrap();

        let mut permissions = fs::metadata(cli.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(cli.clone(), permissions).unwrap();
    }
    EasytierFactory { exe, cli }
}

impl Drop for EasytierFactory {
    fn drop(&mut self) {
        self.remove();
    }
}

impl EasytierFactory {
    pub fn create(&self, args: Vec<String>) -> Easytier {
        fs::metadata(&self.exe).unwrap();

        let rpc = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|socket| socket.local_addr())
            .map(|address| address.port())
            .unwrap_or(35785);

        logging!("Easytier", "Starting easytier: {:?}, rpc={}", args, rpc);

        let mut process = Command::new(self.exe.as_path());
        process
            .args(args)
            .args(["-r", &rpc.to_string()])
            .current_dir(env::temp_dir())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_family = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x08000000);
        }

        let mut process = process.spawn().unwrap();

        let (sender, receiver) = mpsc::channel::<String>();
        let pump = [
            (
                "stdout",
                Self::pump_std(&sender, process.stdout.take().unwrap()),
            ),
            (
                "stderr",
                Self::pump_std(&sender, process.stderr.take().unwrap()),
            ),
        ];

        let process: Arc<Mutex<Child>> = Arc::new(Mutex::new(process));
        let process2 = process.clone();

        thread::spawn(move || {
            const LINES: usize = 500;

            let mut buffer: [Option<String>; LINES] = [const { None }; LINES];
            let mut index = 0;

            let status = loop {
                {
                    let mut process = process2.lock().unwrap();
                    if let Ok(value) = process.try_wait() {
                        if let Some(_) = value {
                            break value;
                        }
                    } else {
                        break None;
                    }
                }

                if let Ok(value) = receiver.try_recv() {
                    buffer[index] = Some(value);
                    index = (index + 1) % LINES;
                }

                thread::sleep(Duration::from_millis(100));
            };

            thread::sleep(Duration::from_secs(3));
            for (name, join) in pump.into_iter() {
                if !join.is_finished() {
                    logging!("UI", "Logging adapter {} has hang for 3s.", name);
                }
            }

            let mut output = String::from("Easytier has exit. with status {");
            output += &match status {
                Some(status) => format!(
                    "code={}, success={}",
                    status
                        .code()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    status.success()
                ),
                None => "unknown".to_string(),
            };
            output.push_str("}. Here's the logs:\n---------------");
            for i in 0..LINES {
                if let Some(value) = &buffer[(index + 1 + i) % LINES] {
                    output.push('\n');
                    output.push_str(&value);
                }
            }
            output.push_str("\n---------------");

            logging!("Easytier", "{}", output);
        });

        Easytier {
            process,
            rpc,
            cli: self.cli.clone(),
        }
    }

    fn pump_std<R: std::io::Read + Send + 'static>(
        sender: &mpsc::Sender<String>,
        source: R,
    ) -> thread::JoinHandle<()> {
        let sender = sender.clone();
        thread::spawn(move || {
            let reader = BufReader::new(source);
            for line in reader.lines().flatten() {
                sender.send(line).unwrap();
            }
        })
    }

    pub fn remove(&self) {
        let dir = self.exe.parent();
        if let Some(dir) = dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl Easytier {
    pub fn kill(self) {
        let _ = self.process.lock().unwrap().kill();
    }

    pub fn is_alive(&mut self) -> bool {
        if let Ok(value) = self.process.lock().unwrap().try_wait() {
            !value.is_some()
        } else {
            false
        }
    }

    pub fn get_players(&mut self) -> Option<Vec<(String, Ipv4Addr)>> {
        let object: serde_json::Value = serde_json::from_str(std::str::from_utf8(
            &self.start_cli()
                .args(["-p", &format!("127.0.0.1:{}", self.rpc), "-o", "json", "peer"])
                .output().ok()?.stdout
        ).ok()?).ok()?;

        let mut players: Vec<(String, Ipv4Addr)> = vec![];
        for item in object.as_array()? {
            let host = item.as_object()?.get("hostname")?.as_str()?.to_string();
            if let Ok(ip) = Ipv4Addr::from_str(item.as_object()?.get("ipv4")?.as_str()?) {
                players.push((host, ip));
            }

        }
        Some(players)
    }

    pub fn add_port_forward(
        &mut self,
        local_ip: IpAddr,
        local_port: u16,
        remote_ip: IpAddr,
        remote_port: u16,
    ) -> bool {
        fn to_string(ip: IpAddr, port: u16) -> String {
            match ip {
                IpAddr::V4(ip) => format!("{}:{}", ip, port),
                IpAddr::V6(ip) => format!("[{}]:{}", ip, port),
            }
        }

        for kind in ["tcp", "udp"] {
            if !self.start_cli()
                .args([
                    "-p", &format!("127.0.0.1:{}", self.rpc), "port-forward", "add",
                    kind, &to_string(local_ip, local_port), &to_string(remote_ip, remote_port),
                ])
                .status()
                .is_ok_and(|status| status.success()) {
                return false;
            }
        }
        true
    }

    fn start_cli(&mut self) -> Command {
        let mut command = Command::new(self.cli.as_path());
        command.current_dir(env::temp_dir());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }
}

impl Drop for Easytier {
    fn drop(&mut self) {
        logging!("EasyTier", "Killing EasyTier.");
        let _ = self.process.lock().unwrap().kill();
    }
}
