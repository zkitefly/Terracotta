use std::{env, fs, path};

fn main() {
    enum TargetTransform {
        NONE,
        TAR,
        DMG,
    }

    struct Target {
        toolchain: &'static str,
        executable: &'static str,
        classifier: &'static str,
        transform: TargetTransform,
    }

    impl Target {
        pub fn locate(&self) -> path::PathBuf {
            return env::current_dir().unwrap().join(format!(
                "target/{}/release/{}",
                self.toolchain, self.executable
            ));
        }

        pub fn open(&self) -> fs::File {
            return fs::OpenOptions::new()
                .read(true)
                .open(self.locate())
                .unwrap();
        }
    }

    let targets: Vec<Target> = vec![
        Target {
            toolchain: "x86_64-pc-windows-gnullvm",
            executable: "terracotta.exe",
            classifier: "windows-x86_64.exe",
            transform: TargetTransform::NONE,
        },
        Target {
            toolchain: "aarch64-pc-windows-gnullvm",
            executable: "terracotta.exe",
            classifier: "windows-aarch64.exe",
            transform: TargetTransform::NONE,
        },
        Target {
            toolchain: "x86_64-unknown-linux-gnu",
            executable: "terracotta",
            classifier: "linux-x86_64-gnu",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "aarch64-unknown-linux-gnu",
            executable: "terracotta",
            classifier: "linux-aarch64-gnu",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "x86_64-unknown-linux-musl",
            executable: "terracotta",
            classifier: "linux-x86_64-musl",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "aarch64-unknown-linux-musl",
            executable: "terracotta",
            classifier: "linux-aarch64-musl",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "x86_64-apple-darwin",
            executable: "terracotta",
            classifier: "macos-x86_64",
            transform: TargetTransform::DMG,
        },
        Target {
            toolchain: "aarch64-apple-darwin",
            executable: "terracotta",
            classifier: "macos-aarch64",
            transform: TargetTransform::DMG,
        },
    ];

    let artifact = env::current_dir()
        .unwrap()
        .join(env::var("TERRACOTTA_ARTIFACT").unwrap());
    if fs::metadata(&artifact).is_ok() {
        fs::remove_dir_all(&artifact).unwrap();
    }
    fs::create_dir_all(&artifact).unwrap();

    for target in targets.iter() {
        let name = format!(
            "terracotta-{}-{}",
            env!("CARGO_PKG_VERSION"),
            target.classifier
        );

        match target.transform {
            TargetTransform::NONE => {
                fs::copy(target.locate(), artifact.join(name)).unwrap();
            }
            TargetTransform::TAR => {
                let mut header = tar::Header::new_gnu();
                header.set_size(target.open().metadata().unwrap().len());
                header.set_cksum();
                tar::Builder::new(
                    fs::File::create(artifact.join(format!("{}.tar", &name))).unwrap(),
                )
                .append_data(&mut header, &name, &mut target.open())
                .unwrap();
            }
            TargetTransform::DMG => {
                let source = env::current_dir().unwrap().join(format!("build/macos"));

                fn copy_dir_all(src: impl AsRef<path::Path>, dst: impl AsRef<path::Path>) {
                    fs::create_dir_all(&dst).unwrap();
                    for entry in fs::read_dir(src).unwrap() {
                        let entry = entry.unwrap();
                        let ty = entry.file_type().unwrap();
                        if ty.is_dir() {
                            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()));
                        } else {
                            fs::copy(entry.path(), dst.as_ref().join(entry.file_name())).unwrap();
                        }
                    }
                }
                copy_dir_all(source, artifact.join(format!("{}/terracotta.app", name)));

                let file = artifact.join(format!("{}/terracotta.app/Contents/MacOS/terracotta", name));
                fs::create_dir_all(&file).unwrap();
                let r = fs::copy(target.locate(),  &file);
                if r.is_err() && fs::metadata(file).is_err() {
                    r.unwrap();
                }
            }
        }
    }
}
