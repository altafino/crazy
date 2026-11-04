use super::*;
use crate::domain::TemplateId;
use crate::persistence::{NewProject, SessionMode, SessionRow};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use tempfile::TempDir;

const DRAFT: &str = "draft-1";

struct Fx {
    dir: TempDir,
    store: AttachmentStore,
    limits: AttachmentLimits,
    pandoc: Pandoc,
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Writes `-o <file>` and a `media/` folder, like pandoc does.
const FAKE_OK: &str = r##"out=""; while [ $# -gt 0 ]; do [ "$1" = "-o" ] && out="$2"; shift; done
mkdir -p media; echo "# Spec ![x](media/image1.png)" > "$out"; echo img > media/image1.png"##;

fn fx(pandoc_body: Option<&str>) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let path = pandoc_body.map(|b| script(dir.path(), "pandoc", b));
    Fx {
        store: AttachmentStore::new(&dir.path().join("data")),
        limits: AttachmentLimits::default(),
        pandoc: Pandoc {
            path,
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
        },
        dir,
    }
}

impl Fx {
    fn file(&self, name: &str, content: &[u8]) -> PathBuf {
        let p = self.dir.path().join(name);
        fs::write(&p, content).unwrap();
        p
    }

    async fn stage(&self, p: &Path) -> Result<StagedAttachment, AttachError> {
        self.store
            .stage_path(DRAFT, p, &self.limits, &self.pandoc)
            .await
    }

    fn staged_dir(&self, a: &StagedAttachment) -> PathBuf {
        self.store.staging().join(DRAFT).join(&a.id)
    }
}

fn png(len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    v[..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
    v
}

fn docx() -> Vec<u8> {
    b"PK\x03\x04 not really a zip".to_vec()
}

fn db_with_project() -> Db {
    let db = Db::open_in_memory().unwrap();
    db.insert_project(&NewProject {
        id: "p1".into(),
        name: "a".into(),
        slug: "a".into(),
        template_id: TemplateId::TauriVue,
        app_id: "com.x.a".into(),
        workspace_path: "/p/a".into(),
        pending_op: None,
    })
    .unwrap();
    db
}

/// Inserts runs in order; `resumed_from` links them into conversations.
fn add_run(db: &Db, id: &str, resumed_from: Option<&str>) {
    db.insert_job(id, "p1", crate::persistence::JobState::Queued)
        .unwrap();
    db.insert_session(&SessionRow {
        id: id.into(),
        job_id: id.into(),
        project_id: "p1".into(),
        profile_id: None,
        mode: SessionMode::Plan,
        prompt: "p".into(),
        provider_session_id: None,
        resumed_from: resumed_from.map(Into::into),
        log_path: "/l".into(),
        before_ref: None,
        after_ref: None,
        checkpoint_ref: None,
        created_at: String::new(),
    })
    .unwrap();
}

// ---------------------------------------------------------------- types

#[tokio::test]
async fn every_listed_type_is_accepted_and_others_are_refused() {
    let f = fx(Some(FAKE_OK));
    for t in supported_types() {
        for ext in &t.extensions {
            let content: Vec<u8> = match ext.as_str() {
                "png" => png(16),
                "jpg" | "jpeg" => vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0],
                "gif" => b"GIF89a....".to_vec(),
                "webp" => b"RIFF\0\0\0\0WEBPVP8 ".to_vec(),
                "pdf" => b"%PDF-1.7 x".to_vec(),
                "docx" => docx(),
                _ => "héllo\n".as_bytes().to_vec(),
            };
            let p = f.file(&format!("sample.{}", ext.to_uppercase()), &content);
            let got = f.stage(&p).await;
            assert!(got.is_ok(), "{ext}: {got:?}");
            assert_eq!(got.unwrap().kind, t.kind);
            // Keep the draft under the file-count limit.
            f.store.map().clear();
        }
    }
    for name in [
        "sheet.xlsx",
        "deck.pptx",
        "doc.odt",
        "calc.ods",
        "noext",
        "a.exe",
    ] {
        let p = f.file(name, b"PK\x03\x04");
        assert_eq!(f.stage(&p).await, Err(AttachError::Unsupported), "{name}");
    }
}

#[tokio::test]
async fn content_must_match_the_extension() {
    let f = fx(None);
    let fake_png = f.file("a.png", b"this is text");
    assert_eq!(f.stage(&fake_png).await, Err(AttachError::Unsupported));
    let binary_txt = f.file("a.txt", &[0xFF, 0xFE, 0x00, 0x01]);
    assert_eq!(f.stage(&binary_txt).await, Err(AttachError::Unsupported));
    assert!(
        !f.store
            .staging()
            .join(DRAFT)
            .read_dir()
            .unwrap()
            .any(|_| true),
        "a refused file leaves nothing behind"
    );
}

// ---------------------------------------------------------------- limits

#[tokio::test]
async fn per_file_limit_at_limit_and_one_byte_over() {
    let mut f = fx(None);
    f.limits.max_image_mb = 1;
    let at = f.file("at.png", &png(1024 * 1024));
    assert!(f.stage(&at).await.is_ok());
    let over = f.file("over.png", &png(1024 * 1024 + 1));
    assert_eq!(f.stage(&over).await, Err(AttachError::TooLarge { mb: 1 }));
    // A changed setting applies on the next attach.
    f.limits.max_image_mb = 2;
    assert!(f.stage(&over).await.is_ok());
}

#[tokio::test]
async fn pasted_bytes_follow_the_same_limits() {
    let mut f = fx(None);
    f.limits.max_image_mb = 1;
    let ok = f
        .store
        .stage_bytes(
            DRAFT,
            "Pasted image.png",
            &png(1024 * 1024),
            &f.limits,
            &f.pandoc,
        )
        .await
        .unwrap();
    assert_eq!(ok.name, "Pasted image.png");
    assert_eq!(ok.stored_path, format!("{}/Pasted_image.png", ok.id));
    let over = f
        .store
        .stage_bytes(DRAFT, "x.png", &png(1024 * 1024 + 1), &f.limits, &f.pandoc)
        .await;
    assert_eq!(over, Err(AttachError::TooLarge { mb: 1 }));
}

#[tokio::test]
async fn file_count_and_total_limits_at_limit_and_over() {
    let mut f = fx(None);
    f.limits.max_files = 2;
    let (a, b, c) = (
        f.file("a.txt", b"a"),
        f.file("b.txt", b"b"),
        f.file("c.txt", b"c"),
    );
    f.stage(&a).await.unwrap();
    f.stage(&b).await.unwrap();
    assert_eq!(f.stage(&c).await, Err(AttachError::TooManyFiles { max: 2 }));
    // Another draft has its own count.
    assert!(
        f.store
            .stage_path("draft-2", &c, &f.limits, &f.pandoc)
            .await
            .is_ok()
    );

    let mut f = fx(None);
    f.limits.max_total_mb = 1;
    let half = vec![b'a'; 512 * 1024];
    let (x, y) = (f.file("x.txt", &half), f.file("y.txt", &half));
    f.stage(&x).await.unwrap();
    f.stage(&y).await.unwrap(); // exactly 1 MB in total
    let z = f.file("z.txt", b"z");
    assert_eq!(f.stage(&z).await, Err(AttachError::TotalTooLarge { mb: 1 }));
    // A refused file does not count; removing a chip frees room.
    let first = f.store.map().values().next().unwrap().info.id.clone();
    f.store.remove_staged(DRAFT, &first);
    assert!(f.stage(&z).await.is_ok());
}

// ---------------------------------------------------------------- secrets, copy-not-link

#[tokio::test]
async fn secret_looking_files_are_refused_even_through_a_symlink() {
    let f = fx(None);
    for name in ["credentials.txt", "id_rsa.md"] {
        let p = f.file(name, b"secret");
        assert_eq!(f.stage(&p).await, Err(AttachError::SecretFile), "{name}");
    }
    let real = f.file("id_ed25519", b"key");
    let link = f.dir.path().join("notes.txt");
    symlink(&real, &link).unwrap();
    assert_eq!(f.stage(&link).await, Err(AttachError::SecretFile));
}

#[tokio::test]
async fn files_are_copied_not_linked() {
    let f = fx(None);
    let real = f.file("real.txt", b"one");
    let link = f.dir.path().join("link.txt");
    symlink(&real, &link).unwrap();
    for src in [&real, &link] {
        f.store.map().clear();
        let a = f.stage(src).await.unwrap();
        let stored = f.store.staging().join(DRAFT).join(&a.stored_path);
        let m = fs::symlink_metadata(&stored).unwrap();
        assert!(m.is_file() && !m.file_type().is_symlink());
        assert_eq!(m.nlink(), 1, "not a hard link");
        assert_eq!(m.permissions().mode() & 0o111, 0, "never executable");
        fs::write(&real, b"changed").unwrap();
        assert_eq!(fs::read(&stored).unwrap(), b"one");
        fs::write(&real, b"one").unwrap();
    }
    let dir = f.dir.path().join("somedir.txt");
    fs::create_dir(&dir).unwrap();
    assert!(matches!(
        f.stage(&dir).await,
        Err(AttachError::CopyFailed { .. })
    ));
}

// ---------------------------------------------------------------- pandoc

#[tokio::test]
async fn docx_is_converted_and_media_travel_with_it() {
    let f = fx(Some(FAKE_OK));
    let p = f.file("My Spec.docx", &docx());
    let a = f.stage(&p).await.unwrap();
    assert_eq!(a.kind, AttachmentKind::Docx);
    let md = a.converted_path.clone().unwrap();
    assert_eq!(md, format!("{}/My_Spec.md", a.id));
    let item = f.staged_dir(&a);
    assert!(
        fs::read_to_string(item.join("My_Spec.md"))
            .unwrap()
            .contains("# Spec")
    );
    assert!(item.join("media/image1.png").is_file());
}

#[tokio::test]
async fn docx_without_pandoc_is_refused_and_nothing_is_staged() {
    let mut f = fx(None);
    let p = f.file("a.docx", &docx());
    assert_eq!(f.stage(&p).await, Err(AttachError::PandocMissing));
    // A configured path that does not exist reads the same.
    f.pandoc.path = Some(f.dir.path().join("no-such-pandoc"));
    assert_eq!(f.stage(&p).await, Err(AttachError::PandocMissing));
    assert!(f.store.map().is_empty());
    assert!(
        !f.store
            .staging()
            .join(DRAFT)
            .read_dir()
            .unwrap()
            .any(|_| true)
    );
    assert_eq!(
        AttachError::PandocMissing.to_string(),
        "Install pandoc to attach Word files"
    );
}

#[tokio::test]
async fn pandoc_timeout_failure_and_oversized_output_are_per_file_errors() {
    let mut f = fx(Some("/bin/sleep 30"));
    f.limits.pandoc_timeout_secs = 1;
    let p = f.file("a.docx", &docx());
    let t = std::time::Instant::now();
    assert_eq!(f.stage(&p).await, Err(AttachError::ConversionTimedOut));
    assert!(t.elapsed() < Duration::from_secs(10));
    assert!(
        !f.store
            .staging()
            .join(DRAFT)
            .read_dir()
            .unwrap()
            .any(|_| true)
    );

    let f = fx(Some("echo 'bad zip' >&2; exit 64"));
    let p = f.file("a.docx", &docx());
    assert_eq!(
        f.stage(&p).await,
        Err(AttachError::ConversionFailed {
            detail: "bad zip".into()
        })
    );

    let mut f = fx(Some(
        r#"out=""; while [ $# -gt 0 ]; do [ "$1" = "-o" ] && out="$2"; shift; done
head -c 1048577 /dev/zero | tr '\0' a > "$out""#,
    ));
    f.limits.max_docx_converted_mb = 1;
    let p = f.file("a.docx", &docx());
    assert_eq!(f.stage(&p).await, Err(AttachError::TooLarge { mb: 1 }));
}

#[test]
fn error_reasons_are_stable_machine_keys() {
    let j = |e: AttachError| serde_json::to_value(e).unwrap();
    assert_eq!(
        j(AttachError::Unsupported),
        serde_json::json!({"reason": "unsupported"})
    );
    assert_eq!(
        j(AttachError::TooLarge { mb: 20 }),
        serde_json::json!({"reason": "tooLarge", "mb": 20})
    );
    assert_eq!(j(AttachError::PandocMissing)["reason"], "pandocMissing");
    assert_eq!(
        j(AttachError::ConversionTimedOut)["reason"],
        "conversionTimedOut"
    );
    assert_eq!(j(AttachError::SecretFile)["reason"], "secretFile");
}

// ---------------------------------------------------------------- staging lifecycle

#[tokio::test]
async fn commit_moves_files_into_the_conversation_folder_and_records_rows() {
    let f = fx(Some(FAKE_OK));
    let db = db_with_project();
    add_run(&db, "s1", None);
    let img = f.stage(&f.file("shot.png", &png(8))).await.unwrap();
    let doc = f.stage(&f.file("spec.docx", &docx())).await.unwrap();
    let ids = vec![img.id.clone(), doc.id.clone()];

    let rows = f.store.commit(&db, DRAFT, "s1", "s1", &ids).unwrap();
    let conv = f.store.dir_for("s1");
    assert!(conv.join(&img.stored_path).is_file());
    assert!(conv.join(doc.converted_path.as_ref().unwrap()).is_file());
    assert!(conv.join(&doc.id).join("media/image1.png").is_file());
    assert!(!f.staged_dir(&img).exists());
    assert!(
        !f.store.staging().join(DRAFT).exists(),
        "empty draft folder is gone"
    );
    assert!(f.store.map().is_empty());
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].ordinal, rows[1].ordinal), (0, 1));

    let stored = db
        .list_attachments(&["s1".to_owned(), "nope".to_owned()])
        .unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].name, "shot.png");
    assert_eq!(stored[1].converted_path, doc.converted_path);
    assert!(!stored[0].removed && !stored[0].created_at.is_empty());
    assert!(db.list_attachments(&[]).unwrap().is_empty());
}

#[tokio::test]
async fn commit_refuses_when_a_staged_file_vanished_and_moves_nothing() {
    let f = fx(None);
    let db = db_with_project();
    add_run(&db, "s1", None);
    let a = f.stage(&f.file("a.txt", b"a")).await.unwrap();
    let b = f.stage(&f.file("b.txt", b"b")).await.unwrap();
    fs::remove_file(f.store.staging().join(DRAFT).join(&b.stored_path)).unwrap();
    let ids = vec![a.id.clone(), b.id.clone()];
    assert_eq!(
        f.store.commit(&db, DRAFT, "s1", "s1", &ids),
        Err(AttachError::FileMissing)
    );
    assert!(f.staged_dir(&a).exists() && !f.store.dir_for("s1").exists());
    assert!(db.list_attachments(&["s1".into()]).unwrap().is_empty());
    // An unknown id, or an id of another draft, reads the same.
    assert_eq!(
        f.store.commit(&db, "other", "s1", "s1", &[a.id]),
        Err(AttachError::FileMissing)
    );
}

#[tokio::test]
async fn removing_a_chip_deletes_its_file() {
    let f = fx(Some(FAKE_OK));
    let a = f.stage(&f.file("a.docx", &docx())).await.unwrap();
    let dir = f.staged_dir(&a);
    assert!(dir.exists());
    f.store.remove_staged("other-draft", &a.id); // wrong draft: ignored
    assert!(dir.exists());
    f.store.remove_staged(DRAFT, &a.id);
    assert!(!dir.exists(), "file, conversion and media are all gone");
    assert!(f.store.map().is_empty());
}

#[tokio::test]
async fn sweep_removes_only_staging_folders_older_than_24_hours() {
    let f = fx(None);
    let old = f.stage(&f.file("a.txt", b"a")).await.unwrap();
    let fresh = f
        .store
        .stage_path("draft-2", &f.file("b.txt", b"b"), &f.limits, &f.pandoc)
        .await
        .unwrap();
    let day_and_a_bit = SystemTime::now() - Duration::from_secs(25 * 3600);
    fs::File::open(f.store.staging().join(DRAFT))
        .unwrap()
        .set_modified(day_and_a_bit)
        .unwrap();
    let conv = f.store.dir_for("conv");
    fs::create_dir_all(&conv).unwrap();
    f.store.sweep_staging(Duration::from_secs(24 * 3600));
    assert!(!f.store.staging().join(DRAFT).exists());
    assert!(f.store.staging().join("draft-2").join(&fresh.id).exists());
    assert!(conv.exists(), "conversation folders are never swept");
    assert!(f.store.map().get(&old.id).is_none() && f.store.map().get(&fresh.id).is_some());
}

// ---------------------------------------------------------------- conversations and prune

#[test]
fn conversation_id_follows_resumed_from_to_the_first_run() {
    let db = db_with_project();
    add_run(&db, "a1", None);
    add_run(&db, "a2", Some("a1"));
    add_run(&db, "a3", Some("a2"));
    add_run(&db, "b1", None);
    add_run(&db, "c2", Some("gone")); // the earlier session need not exist
    for (run, want) in [("a1", "a1"), ("a3", "a1"), ("b1", "b1"), ("c2", "gone")] {
        assert_eq!(conversation_id(&db, run).unwrap(), want);
    }
}

fn attach_to(f: &Fx, db: &Db, conv: &str, run: &str) {
    let dir = f.store.dir_for(conv).join("item");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.txt"), "x").unwrap();
    db.insert_attachments(&[AttachmentRow {
        session_id: run.into(),
        ordinal: 0,
        name: "a.txt".into(),
        stored_path: "item/a.txt".into(),
        kind: AttachmentKind::Text,
        size_bytes: 1,
        converted_path: None,
        removed: false,
        created_at: String::new(),
    }])
    .unwrap();
}

#[test]
fn prune_keeps_a_conversation_with_a_recent_run_and_removes_the_rest() {
    let f = fx(None);
    let db = db_with_project();
    add_run(&db, "a1", None);
    add_run(&db, "b1", None);
    add_run(&db, "a2", Some("a1")); // newest run continues conversation A
    attach_to(&f, &db, "a1", "a1");
    attach_to(&f, &db, "b1", "b1");

    f.store.prune(&db, "p1", 1).unwrap();
    assert!(
        f.store.dir_for("a1").exists(),
        "A has a run among the last 1"
    );
    assert!(!f.store.dir_for("b1").exists());
    let rows = db.list_attachments(&["a1".into(), "b1".into()]).unwrap();
    assert!(
        !rows[0].removed && rows[1].removed,
        "pruned rows say 'File removed'"
    );

    f.store.prune(&db, "p1", 20).unwrap();
    assert!(f.store.dir_for("a1").exists());

    f.store.remove_project(&db, "p1").unwrap();
    assert!(!f.store.dir_for("a1").exists());
}

#[test]
fn staging_folder_is_matched_by_the_scopes_double_star() {
    // Tauri scopes don't let `**` match a component starting with `.` on Unix.
    assert!(!super::STAGING.starts_with('.'));
}
