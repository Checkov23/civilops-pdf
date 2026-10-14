/// CivilOps PDF: built with the GNU toolchain, this manifest takes the place of MinGW's default
/// one (`default-manifest.o`, which ld keeps behind it), so it also says what that one says: long
/// paths and `asInvoker`.
pub(crate) const WINDOWS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </asmv3:windowsSettings>
  </asmv3:application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

#[cfg(test)]
mod tests {
    use super::WINDOWS_MANIFEST;

    #[test]
    fn declares_per_monitor_v2_with_per_monitor_fallback() {
        assert!(
            WINDOWS_MANIFEST
                .contains("<dpiAwareness xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">PerMonitorV2, PerMonitor</dpiAwareness>")
        );
    }

    #[test]
    fn legacy_declaration_is_per_monitor_aware() {
        assert!(WINDOWS_MANIFEST.contains("<dpiAware xmlns=\"http://schemas.microsoft.com/SMI/2005/WindowsSettings\">true/pm</dpiAware>"));
    }

    #[test]
    fn keeps_what_the_mingw_default_manifest_declares() {
        assert!(WINDOWS_MANIFEST.contains("<longPathAware xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">true</longPathAware>"));
        assert!(WINDOWS_MANIFEST.contains("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"false\"/>"));
    }

    #[test]
    fn windows_resource_build_script_embeds_this_manifest() {
        let build_script = include_str!("../build.rs");
        assert!(build_script.contains("set_manifest(windows_manifest::WINDOWS_MANIFEST)"));
    }
}
