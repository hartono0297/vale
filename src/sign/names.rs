//! Where a signed copy goes: next to the original, numbered when the name is
//! taken, or in Documents when the folder cannot be written.

use std::path::{Path, PathBuf};

use crate::i18n::Lang;

/// `<stem> (ditandatangani).<ext>` (or `(signed)`) in the source's folder;
/// ` (2)`..` (999)` when taken, then ` (1000)`. A folder `writable` rejects
/// sends it to `fallback` with the same naming.
pub fn output_path(
    src: &Path,
    ext: &str,
    lang: Lang,
    exists: impl Fn(&Path) -> bool,
    writable: impl Fn(&Path) -> bool,
    fallback: &Path,
) -> PathBuf {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Dokumen".to_owned());
    let word = match lang {
        Lang::Id => "ditandatangani",
        Lang::En => "signed",
    };
    let base = format!("{stem} ({word})");
    let dir = match src.parent() {
        Some(d) if d.as_os_str().is_empty() => Path::new("."),
        Some(d) if writable(d) => d,
        _ => fallback,
    };
    let name = |suffix: String| dir.join(format!("{base}{suffix}.{ext}"));
    let first = name(String::new());
    if !exists(&first) {
        return first;
    }
    (2..1000)
        .map(|n| name(format!(" ({n})")))
        .find(|p| !exists(p))
        .unwrap_or_else(|| name(" (1000)".to_owned()))
}

/// `Documents\Vale\Ditandatangani`; under the temp folder when Windows cannot
/// say where Documents is.
pub fn fallback_dir() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let docs = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None)
            .ok()
            .and_then(|raw| {
                let p = raw.to_string().ok().map(PathBuf::from);
                CoTaskMemFree(Some(raw.0 as *const _));
                p
            })
            .filter(|p| p.is_absolute())
    };
    docs.unwrap_or_else(std::env::temp_dir)
        .join("Vale")
        .join("Ditandatangani")
}

/// Whether a file can be created in `dir`: a probe file is made and removed.
pub fn dir_is_writable(dir: &Path) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let probe = dir.join(format!(
        ".vale-probe-{}-{}-{nanos}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let ok = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if ok {
        let _ = std::fs::remove_file(&probe);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fb() -> PathBuf {
        PathBuf::from("C:/fb")
    }

    #[test]
    fn id_name() {
        let p = output_path(
            Path::new("C:/d/Kontrak sewa.pdf"),
            "pdf",
            Lang::Id,
            |_| false,
            |_| true,
            &fb(),
        );
        assert_eq!(p, PathBuf::from("C:/d/Kontrak sewa (ditandatangani).pdf"));
    }

    #[test]
    fn en_name() {
        let p = output_path(
            Path::new("C:/d/Kontrak sewa.pdf"),
            "pdf",
            Lang::En,
            |_| false,
            |_| true,
            &fb(),
        );
        assert_eq!(p, PathBuf::from("C:/d/Kontrak sewa (signed).pdf"));
    }

    #[test]
    fn numbers_when_taken() {
        let taken = |p: &Path| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            n == "Kontrak sewa (ditandatangani).pdf" || n == "Kontrak sewa (ditandatangani) (2).pdf"
        };
        let p = output_path(
            Path::new("C:/d/Kontrak sewa.pdf"),
            "pdf",
            Lang::Id,
            taken,
            |_| true,
            &fb(),
        );
        assert_eq!(
            p,
            PathBuf::from("C:/d/Kontrak sewa (ditandatangani) (3).pdf")
        );
    }

    #[test]
    fn image_keeps_or_changes_ext() {
        let src = Path::new("C:/d/Surat.JPG");
        let same = output_path(src, "jpg", Lang::Id, |_| false, |_| true, &fb());
        assert_eq!(same, PathBuf::from("C:/d/Surat (ditandatangani).jpg"));
        let pdf = output_path(src, "pdf", Lang::Id, |_| false, |_| true, &fb());
        assert_eq!(pdf, PathBuf::from("C:/d/Surat (ditandatangani).pdf"));
    }

    #[test]
    fn falls_back_when_folder_is_read_only() {
        let p = output_path(
            Path::new("C:/d/Kontrak sewa.pdf"),
            "pdf",
            Lang::Id,
            |_| false,
            |_| false,
            &fb(),
        );
        assert_eq!(p, fb().join("Kontrak sewa (ditandatangani).pdf"));
    }

    #[test]
    fn office_source_names_from_original() {
        let p = output_path(
            Path::new("C:/d/Penawaran.docx"),
            "pdf",
            Lang::Id,
            |_| false,
            |_| true,
            &fb(),
        );
        assert_eq!(p, PathBuf::from("C:/d/Penawaran (ditandatangani).pdf"));
    }

    #[test]
    fn numbers_inside_the_fallback_dir_too() {
        let taken = |p: &Path| p == fb().join("Kontrak sewa (ditandatangani).pdf");
        let p = output_path(
            Path::new("C:/d/Kontrak sewa.pdf"),
            "pdf",
            Lang::Id,
            taken,
            |_| false,
            &fb(),
        );
        assert_eq!(p, fb().join("Kontrak sewa (ditandatangani) (2).pdf"));
    }

    #[test]
    fn probe_detects_a_writable_folder() {
        let dir = crate::phone::files::tests::temp_dir("sign-probe");
        assert!(dir_is_writable(&dir));
        assert!(!dir_is_writable(&dir.join("missing")));
    }
}
