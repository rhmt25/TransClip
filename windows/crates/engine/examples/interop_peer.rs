//! A real engine peer for the Kotlin interop tests (task P5-02).
//!
//! Usage: `interop_peer <group-key-hex> <receive-dir> [send-path]`
//!
//! Prints `PORT <n>` once listening, `ONLINE <peer-hex>` when a peer connects and
//! `BATCH <ok> <failed> <in|out>` after every finished batch. With `send-path` the folder or
//! file is sent to the first peer that comes online.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use engine::link::LocalInfo;
use engine::peers::{ManagerConfig, ManagerEvent, PeerManager};
use engine::transfer::{scan, FsStore, TransferConfig, TransferEvent, TransferManager};
use proto::group::GroupKey;
use proto::ids::DeviceId;
use tokio::sync::mpsc;

fn parse_key(hex: &str) -> Option<GroupKey> {
    if hex.len() != 64 {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(GroupKey(k))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(key_hex), Some(dir)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: interop_peer <group-key-hex> <receive-dir> [send-path]");
        std::process::exit(2);
    };
    let Some(gk) = parse_key(key_hex) else {
        eprintln!("bad group key");
        std::process::exit(2);
    };
    let send_path = args.get(3).map(PathBuf::from);

    let local = LocalInfo {
        id: DeviceId([0x5a; 16]),
        name: "interop-peer".into(),
        platform: "windows".into(),
        app: "interop".into(),
        caps: vec!["clip".into(), "file".into()],
        role: "full".into(),
    };
    let listen = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
    let (_seen_tx, seen_rx) = mpsc::channel(4);
    let Ok((mgr, mut events)) =
        PeerManager::start(ManagerConfig::new(local, gk, listen), seen_rx).await
    else {
        eprintln!("cannot listen");
        std::process::exit(1);
    };
    let mgr = Arc::new(mgr);
    let (tm, mut tm_events) = TransferManager::new(
        Arc::clone(&mgr),
        Arc::new(FsStore::new(dir)),
        TransferConfig::default(),
    );
    println!("PORT {}", mgr.local_addr().port());

    tokio::spawn(async move {
        while let Some(ev) = tm_events.recv().await {
            if let TransferEvent::BatchFinished {
                ok,
                failed,
                incoming,
                ..
            } = ev
            {
                println!(
                    "BATCH {ok} {failed} {}",
                    if incoming { "in" } else { "out" }
                );
            }
        }
    });

    let mut sent = false;
    while let Some(ev) = events.recv().await {
        if let ManagerEvent::Online(info) = &ev {
            println!("ONLINE {}", info.id.to_hex());
            if let (Some(path), false) = (&send_path, sent) {
                sent = true;
                if let Ok(batches) = scan::batch_from_paths(std::slice::from_ref(path)) {
                    let peer = info.id;
                    for b in batches {
                        let tm = tm.clone();
                        tokio::spawn(async move {
                            // Give the peer a moment to finish its own link set-up.
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            let _ = tm.send_batch(peer, b).await;
                        });
                    }
                }
            }
        }
        let _ = tm.handle_event(ev).await;
    }
}
