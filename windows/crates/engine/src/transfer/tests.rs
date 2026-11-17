//! Integration tests: two engines on localhost.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use proto::beacon::Platform;
use proto::group::GroupKey;
use proto::ids::DeviceId;
use proto::msg::{EntryKind, Message};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::scan::batch_from_paths;
use super::*;
use crate::discovery::{PeerSeen, Via};
use crate::link::LocalInfo;
use crate::peers::ManagerConfig;

struct Node {
    id: DeviceId,
    mgr: Arc<PeerManager>,
    tm: TransferManager,
    events: mpsc::Receiver<TransferEvent>,
    seen: mpsc::Sender<PeerSeen>,
    pump: JoinHandle<()>,
}

fn mgr_cfg(id: u8, listen: SocketAddr) -> ManagerConfig {
    let local = LocalInfo {
        id: DeviceId([id; 16]),
        name: format!("dev{id}"),
        platform: "windows".into(),
        app: "test".into(),
        caps: vec![],
        role: "full".into(),
    };
    let mut c = ManagerConfig::new(local, GroupKey([7; 32]), listen);
    c.backoff_min = Duration::from_millis(50);
    c.backoff_max = Duration::from_millis(200);
    c.fallback_after = Duration::from_secs(3600);
    c
}

fn temp(tag: &str) -> PathBuf {
    // Tests run in parallel inside one process, so every call gets its own folder.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tc-xfer-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn node(id: u8, port: u16, dest: &Path, cfg: TransferConfig) -> Node {
    let (seen_tx, seen_rx) = mpsc::channel(16);
    let listen = SocketAddr::from(([127, 0, 0, 1], port));
    let (mgr, mut mgr_events) = PeerManager::start(mgr_cfg(id, listen), seen_rx)
        .await
        .unwrap();
    let mgr = Arc::new(mgr);
    let (tm, events) = TransferManager::new(Arc::clone(&mgr), Arc::new(FsStore::new(dest)), cfg);
    let tm2 = tm.clone();
    let pump = tokio::spawn(async move {
        while let Some(ev) = mgr_events.recv().await {
            let _ = tm2.handle_event(ev).await;
        }
    });
    Node {
        id: DeviceId([id; 16]),
        mgr,
        tm,
        events,
        seen: seen_tx,
        pump,
    }
}

async fn introduce(a: &Node, b: &Node) {
    a.seen
        .send(PeerSeen {
            device_id: b.id,
            addr: b.mgr.local_addr(),
            platform: Platform::Windows,
            name: "b".into(),
            via: Via::Beacon,
        })
        .await
        .unwrap();
}

async fn online(n: &Node) {
    for _ in 0..100 {
        if !n.mgr.online_peers().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("peer did not come online");
}

async fn pair() -> (Node, Node, PathBuf, PathBuf) {
    pair_with(TransferConfig::default(), TransferConfig::default()).await
}

async fn pair_with(cfg_a: TransferConfig, cfg_b: TransferConfig) -> (Node, Node, PathBuf, PathBuf) {
    let (src, dst) = (temp("src"), temp("dst"));
    let a = node(1, 0, &src, cfg_a).await;
    let b = node(2, 0, &dst, cfg_b).await;
    introduce(&a, &b).await;
    online(&a).await;
    online(&b).await;
    (a, b, src, dst)
}

fn same_tree(a: &Path, b: &Path) {
    let mut left: Vec<_> = walk(a);
    let mut right: Vec<_> = walk(b);
    left.sort();
    right.sort();
    assert_eq!(left.len(), right.len(), "{left:?} vs {right:?}");
    for (l, r) in left.iter().zip(&right) {
        assert_eq!(l.strip_prefix(a).unwrap(), r.strip_prefix(b).unwrap());
        if l.is_file() {
            assert_eq!(std::fs::read(l).unwrap(), std::fs::read(r).unwrap());
        }
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        out.push(e.path());
        if e.path().is_dir() {
            out.extend(walk(&e.path()));
        }
    }
    out
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
}

#[tokio::test]
async fn three_level_folder_arrives_identical() {
    let (a, b, src, dst) = pair().await;
    let root = src.join("Foto liburan");
    std::fs::create_dir_all(root.join("a/b/c")).unwrap();
    std::fs::create_dir_all(root.join("kosong")).unwrap();
    std::fs::write(
        root.join("a/b/c/\u{65e5}\u{672c} \u{1f600}.bin"),
        pattern(200_000),
    )
    .unwrap();
    std::fs::write(root.join("a/dua.txt"), b"dua").unwrap();
    std::fs::write(root.join("nol.dat"), b"").unwrap();
    std::fs::write(root.join("top.bin"), pattern(1_000_000)).unwrap();

    let batches = batch_from_paths(std::slice::from_ref(&root)).unwrap();
    let report =
        a.tm.send_batch(b.id, batches.into_iter().next().unwrap())
            .await
            .unwrap();
    // 4 directories (a, a/b, a/b/c, kosong) and 4 files.
    assert_eq!((report.ok, report.failed), (8, 0), "{report:?}");
    same_tree(&root, &dst.join("Foto liburan"));
    assert!(
        dst.join("Foto liburan/kosong").is_dir(),
        "empty folder must arrive"
    );
    assert!(walk(&dst)
        .iter()
        .all(|p| !p.to_string_lossy().ends_with(PART_SUFFIX)));
}

#[tokio::test]
async fn loose_files_land_in_the_root_and_name_conflicts_get_a_counter() {
    let (a, b, src, dst) = pair().await;
    let f = src.join("catatan.txt");
    std::fs::write(&f, b"isi").unwrap();
    for _ in 0..2 {
        let batches = batch_from_paths(std::slice::from_ref(&f)).unwrap();
        let r =
            a.tm.send_batch(b.id, batches.into_iter().next().unwrap())
                .await
                .unwrap();
        assert_eq!((r.ok, r.failed), (1, 0));
    }
    assert_eq!(std::fs::read(dst.join("catatan.txt")).unwrap(), b"isi");
    assert_eq!(std::fs::read(dst.join("catatan (1).txt")).unwrap(), b"isi");
}

#[tokio::test]
async fn hostile_paths_are_rejected() {
    let dst = temp("hostile-dst");
    let b = node(2, 0, &dst, TransferConfig::default()).await;
    // A bare manager plays the attacker so no sender-side validation gets in the way.
    let (seen_tx, seen_rx) = mpsc::channel(4);
    let (evil, mut evil_events) =
        PeerManager::start(mgr_cfg(1, SocketAddr::from(([127, 0, 0, 1], 0))), seen_rx)
            .await
            .unwrap();
    seen_tx
        .send(PeerSeen {
            device_id: b.id,
            addr: b.mgr.local_addr(),
            platform: Platform::Windows,
            name: "b".into(),
            via: Via::Beacon,
        })
        .await
        .unwrap();
    for _ in 0..100 {
        if !evil.online_peers().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for bad in ["../x", "/etc/passwd", "a//b", "CON", "a\\b", "a/./b", "x."] {
        let xfer = format!("00000000-0000-4000-8000-{:012}", bad.len());
        let offer = Message::FileOffer {
            xfer_id: xfer.clone(),
            batch_id: "11111111-1111-4111-8111-111111111111".into(),
            batch_name: String::new(),
            batch_count: 1,
            batch_bytes: 1,
            kind: EntryKind::File,
            relative_path: bad.into(),
            bytes: 1,
            sha256: "0".repeat(64),
            mtime: 0,
        };
        evil.send(&b.id, offer, vec![]).await.unwrap();
        let reply = loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evil_events.recv())
                .await
                .unwrap()
                .unwrap();
            if let crate::peers::ManagerEvent::Frame { frame, .. } = ev {
                break frame.message;
            }
        };
        assert!(
            matches!(&reply, Message::FileReject { reason, .. } if reason == "path"),
            "{bad:?} got {reply:?}"
        );
    }
    // A batch name with a path separator is refused too.
    let offer = Message::FileOffer {
        xfer_id: "00000000-0000-4000-8000-0000000000aa".into(),
        batch_id: "22222222-2222-4222-8222-222222222222".into(),
        batch_name: "../evil".into(),
        batch_count: 1,
        batch_bytes: 1,
        kind: EntryKind::File,
        relative_path: "ok.txt".into(),
        bytes: 1,
        sha256: "0".repeat(64),
        mtime: 0,
    };
    evil.send(&b.id, offer, vec![]).await.unwrap();
    let reply = loop {
        if let crate::peers::ManagerEvent::Frame { frame, .. } =
            tokio::time::timeout(Duration::from_secs(3), evil_events.recv())
                .await
                .unwrap()
                .unwrap()
        {
            break frame.message;
        }
    };
    assert!(matches!(reply, Message::FileReject { .. }));
    assert!(
        std::fs::read_dir(&dst).unwrap().next().is_none(),
        "nothing may be written"
    );
}

#[tokio::test]
async fn disabled_and_oversized_files_are_refused() {
    let off = TransferConfig {
        receive_files: false,
        ..TransferConfig::default()
    };
    let (a, b, src, dst) = pair_with(TransferConfig::default(), off).await;
    let f = src.join("x.bin");
    std::fs::write(&f, pattern(1000)).unwrap();
    let batch = batch_from_paths(std::slice::from_ref(&f))
        .unwrap()
        .remove(0);
    let r = a.tm.send_batch(b.id, batch).await.unwrap();
    assert_eq!((r.ok, r.failed), (0, 1));
    assert!(walk(&dst).is_empty());

    let small = TransferConfig {
        max_file_bytes: 100,
        ..TransferConfig::default()
    };
    let (a, b, src, dst) = pair_with(TransferConfig::default(), small).await;
    let f = src.join("big.bin");
    std::fs::write(&f, pattern(1000)).unwrap();
    let batch = batch_from_paths(&[f]).unwrap().remove(0);
    let r = a.tm.send_batch(b.id, batch).await.unwrap();
    assert_eq!((r.ok, r.failed), (0, 1));
    assert!(walk(&dst).is_empty());
}

#[tokio::test]
async fn interrupted_transfer_resumes_from_the_part_file() {
    let slow = TransferConfig {
        chunk_delay: Duration::from_millis(2),
        reconnect_wait: Duration::from_secs(20),
        ..TransferConfig::default()
    };
    let src = temp("resume-src");
    let dst = temp("resume-dst");
    let mut a = node(1, 0, &src, slow).await;
    let b = node(2, 0, &dst, TransferConfig::default()).await;
    let b_addr = b.mgr.local_addr();
    introduce(&a, &b).await;
    online(&a).await;

    let data = pattern(24 * 1024 * 1024);
    let f = src.join("besar.bin");
    std::fs::write(&f, &data).unwrap();
    let batch = batch_from_paths(&[f]).unwrap().remove(0);
    let tm = a.tm.clone();
    let b_id = b.id;
    let sending = tokio::spawn(async move { tm.send_batch(b_id, batch).await });

    // Wait until a good part of the file is on the receiver, then kill the receiver.
    let part = dst.join(format!("besar.bin{PART_SUFFIX}"));
    for _ in 0..200 {
        if part.metadata().map_or(0, |m| m.len()) > 2 * 1024 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let partial = part.metadata().unwrap().len();
    assert!(
        partial > 0 && partial < data.len() as u64,
        "part is {partial} bytes"
    );
    b.pump.abort();
    b.mgr.stop().await;
    drop(b);

    // A new receiver on the same port and folder; the sender reconnects and resumes.
    let mut port_free = None;
    for _ in 0..50 {
        if let Ok(l) = tokio::net::TcpListener::bind(b_addr).await {
            drop(l);
            port_free = Some(b_addr.port());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut b2 = node(2, port_free.unwrap(), &dst, TransferConfig::default()).await;
    let report = tokio::time::timeout(Duration::from_secs(60), sending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!((report.ok, report.failed), (1, 0), "{report:?}");
    assert_eq!(std::fs::read(dst.join("besar.bin")).unwrap(), data);
    assert!(!part.exists());

    // The second receiver started the entry at a non-zero offset.
    let mut resumed = false;
    while let Ok(ev) = b2.events.try_recv() {
        if let TransferEvent::Started {
            offset,
            incoming: true,
            ..
        } = ev
        {
            resumed |= offset > 0;
        }
    }
    assert!(resumed, "receiver must have resumed from the existing part");
    drop(a.events.try_recv());
}

#[tokio::test]
async fn cancelling_removes_the_part_and_leaves_no_file() {
    let slow = TransferConfig {
        chunk_delay: Duration::from_millis(2),
        ..TransferConfig::default()
    };
    let (mut a, b, src, dst) = pair_with(slow, TransferConfig::default()).await;
    let f = src.join("lama.bin");
    std::fs::write(&f, pattern(16 * 1024 * 1024)).unwrap();
    let batch = batch_from_paths(&[f]).unwrap().remove(0);
    let tm = a.tm.clone();
    let b_id = b.id;
    let sending = tokio::spawn(async move { tm.send_batch(b_id, batch).await });

    let batch_id = loop {
        let ev = tokio::time::timeout(Duration::from_secs(5), a.events.recv())
            .await
            .unwrap()
            .unwrap();
        if let TransferEvent::Started { batch_id, .. } = ev {
            break batch_id;
        }
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    a.tm.cancel_batch(&batch_id);
    let report = sending.await.unwrap().unwrap();
    assert_eq!((report.ok, report.failed), (0, 1));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(walk(&dst).is_empty(), "left over: {:?}", walk(&dst));
}
