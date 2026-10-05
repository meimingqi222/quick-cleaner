fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=rules");
    let mut paths: Vec<_> = std::fs::read_dir("rules")
        .expect("rules directory")
        .map(|entry| entry.expect("rule file").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let definitions: Vec<serde_json::Value> = paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).expect("read rule");
            let value: toml::Value = toml::from_str(&text).expect("parse rule TOML");
            serde_json::to_value(value).expect("encode rule")
        })
        .collect();
    let schema: u32 = std::fs::read_to_string("rules/schema-version")
        .expect("read schema version")
        .trim()
        .parse()
        .expect("numeric schema version");
    let bundle = serde_json::json!({"schema":schema,"sequence":1,"rules":definitions});
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        output.join("rule_schema.rs"),
        format!("pub const SCHEMA: u32 = {schema};\n"),
    )
    .expect("embed schema version");
    std::fs::write(
        output.join("rules.json"),
        serde_json::to_vec(&bundle).unwrap(),
    )
    .expect("embed rules");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=assets/icon.icns");
        println!("cargo:rerun-if-changed=assets/icon-512.png");
        println!("cargo:rerun-if-changed=assets/icon.png");
    }

    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");

        let mut res = winres::WindowsResource::new();
        if std::path::Path::new("assets/icon.ico").exists() {
            res.set_icon("assets/icon.ico");
        }
        res.set("FileDescription", "QuickCleaner 极速磁盘分析与清理工具");
        res.set("ProductName", "QuickCleaner");
        res.set("OriginalFilename", "quick-cleaner.exe");
        res.set("InternalName", "quick-cleaner");
        res.set("LegalCopyright", "Copyright © 2026 QuickCleaner");
        res.set_manifest(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
    <application xmlns="urn:schemas-microsoft-com:asm.v3">
        <windowsSettings>
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
            <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
        </windowsSettings>
    </application>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity
                type="win32"
                name="Microsoft.Windows.Common-Controls"
                version="6.0.0.0"
                processorArchitecture="*"
                publicKeyToken="6595b64144ccf1df"
                language="*"
            />
        </dependentAssembly>
    </dependency>
</assembly>
"#);
        if let Err(e) = res.compile() {
            eprintln!("winres error: {e}");
        }
    }
}
