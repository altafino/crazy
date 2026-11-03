//! Cleaning the environment before spawning host tools from an AppImage (PRD 11, D17).
use std::path::Path;

/// Variables the AppImage launcher adds.
// measured from the Tauri 2.11 linuxdeploy GTK hook; recheck when the bundler changes
const LAUNCHER_VARS: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "APPIMAGE_GTK_THEME",
    "GTK_THEME",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_PATH",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_MODULE_DIR",
    "GI_TYPELIB_PATH",
    "GSETTINGS_SCHEMA_DIR",
];

/// Environment for host tools: `base` unchanged unless running inside an AppImage.
pub fn host_env(base: &[(String, String)]) -> Vec<(String, String)> {
    let get = |name: &str| {
        base.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if get("APPIMAGE").is_none() && get("APPDIR").is_none() {
        return base.to_vec();
    }
    let appdir = get("APPDIR").filter(|d| !d.is_empty()).map(Path::new);
    base.iter()
        .filter(|(k, _)| !LAUNCHER_VARS.contains(&k.as_str()))
        .filter_map(|(k, v)| match appdir {
            // Any variable pointing into the mount is a list the launchers prepended to
            // (LD_LIBRARY_PATH, GST_PLUGIN_SYSTEM_PATH_1_0, PYTHONPATH, ...): drop those entries.
            // Values with no such entry (DISPLAY=":0") are left exactly as they are.
            Some(dir) if v.split(':').any(|e| Path::new(e).starts_with(dir)) => {
                let kept: Vec<&str> = v
                    .split(':')
                    // An empty entry means the current directory; AppRun leaves a trailing one.
                    .filter(|e| !e.is_empty() && !Path::new(e).starts_with(dir))
                    .collect();
                let kept = kept.join(":");
                // An emptied list is removed so the default applies (PATH is always kept).
                (k == "PATH" || !kept.is_empty()).then(|| (k.clone(), kept))
            }
            _ => Some((k.clone(), v.clone())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn unchanged_outside_appimage() {
        let base = env(&[("PATH", "/usr/bin"), ("LD_LIBRARY_PATH", "/x")]);
        assert_eq!(host_env(&base), base);
    }

    #[test]
    fn strips_launcher_vars_and_appdir_entries() {
        let base = env(&[
            ("APPDIR", "/tmp/.mount_x"),
            ("APPIMAGE", "/home/u/Studio.AppImage"),
            ("GTK_PATH", "/tmp/.mount_x/usr/lib/gtk"),
            ("PATH", "/tmp/.mount_x/usr/bin:/usr/bin:/tmp/.mount_xyz/bin"),
            ("XDG_DATA_DIRS", "/tmp/.mount_x/usr/share:/usr/share"),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(
            host_env(&base),
            env(&[
                ("PATH", "/usr/bin:/tmp/.mount_xyz/bin"),
                ("XDG_DATA_DIRS", "/usr/share"),
                ("HOME", "/home/u"),
            ])
        );
    }

    /// The AppImageKit `AppRun` binary prepends `$APPDIR` entries to these. A preview's
    /// `cargo run` loaded the bundled, path-patched WebKit through LD_LIBRARY_PATH and failed
    /// to spawn "././/lib/webkit2gtk-4.1/WebKitNetworkProcess".
    #[test]
    fn strips_apprun_library_and_plugin_paths() {
        let base = env(&[
            ("APPDIR", "/m"),
            ("LD_LIBRARY_PATH", "/m/usr/lib/:/m/lib64/:/opt/cuda/lib64"),
            ("GST_PLUGIN_SYSTEM_PATH", "/m/usr/lib/gstreamer:"),
            ("PYTHONHOME", "/m/usr/"),
            ("PYTHONPATH", "/m/usr/share/pyshared/:"),
            ("PERLLIB", "/m/usr/share/perl5/:/m/usr/lib/perl5/:"),
            ("QT_PLUGIN_PATH", "/m/usr/lib/qt5/plugins/:"),
            ("PATH", "/m/usr/bin/:/usr/bin:"),
        ]);
        assert_eq!(
            host_env(&base),
            env(&[("LD_LIBRARY_PATH", "/opt/cuda/lib64"), ("PATH", "/usr/bin")])
        );
    }

    #[test]
    fn any_variable_pointing_into_the_mount_is_cleaned_and_others_are_untouched() {
        let base = env(&[
            ("APPDIR", "/m"),
            ("GST_PLUGIN_SYSTEM_PATH_1_0", "/m/usr/lib/gstreamer-1.0:"),
            ("SOME_NEW_LIST", "/m/x:/opt/y"),
            ("DISPLAY", ":0"),
            ("MY_LIST", "/a::/b"),
        ]);
        assert_eq!(
            host_env(&base),
            env(&[
                ("SOME_NEW_LIST", "/opt/y"),
                ("DISPLAY", ":0"),
                ("MY_LIST", "/a::/b"),
            ])
        );
    }

    #[test]
    fn emptied_xdg_data_dirs_is_removed() {
        let base = env(&[("APPDIR", "/a"), ("XDG_DATA_DIRS", "/a/usr/share")]);
        assert_eq!(host_env(&base), env(&[]));
    }

    #[test]
    fn appimage_without_appdir_only_strips_vars() {
        let base = env(&[
            ("APPIMAGE", "/a"),
            ("PATH", "/usr/bin"),
            ("GTK_THEME", "/p"),
        ]);
        assert_eq!(host_env(&base), env(&[("PATH", "/usr/bin")]));
    }
}
