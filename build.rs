use std::{env, path::Path};

extern crate winres;

fn main() {
    sevenz_rust2::compress_to_path(
            "web",
            Path::new(&env::var_os("OUT_DIR").unwrap()).join("__webstatics.7z"),
        )
        .unwrap();
    println!("cargo:rerun-if-changed=web");

    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon_with_id(
            Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("icon.ico")
                .to_str()
                .unwrap(), "icon"
        );
        res.set_manifest(
            r#"
          <assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
          <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
              <security>
                  <requestedPrivileges>
                      <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
                  </requestedPrivileges>
              </security>
          </trustInfo>
          </assembly>
          "#,
        );
        res.compile().unwrap();

        match std::env::var("CARGO_CFG_TARGET_ENV").unwrap().as_str() {
            "gnu" => println!("cargo:rustc-link-arg={}", Path::new(&env::var("OUT_DIR").unwrap()).join("resource.o").to_str().unwrap()),
            "msvc" => println!("cargo:rustc-link-arg={}", Path::new(&env::var("OUT_DIR").unwrap()).join("resource.res").to_str().unwrap()),
            _ => panic!()
        }
        println!("cargo:rerun-if-changed=icon.ico");
    }
}
