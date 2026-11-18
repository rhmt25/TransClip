//! Bluetooth RFCOMM transport (PLAN section 4.2) through WinRT.
//!
//! The WinRT socket is wrapped into a tokio duplex stream by two helper threads, so the engine
//! sees an ordinary `AsyncRead + AsyncWrite` and runs its handshake over it unchanged.

use std::io;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;

use engine::peers::{BoxedStream, BtConnectFuture, BtDialer, PeerManager};
use proto::ids::DeviceId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use windows::core::{AgileReference, GUID, HSTRING};
use windows::Devices::Bluetooth::BluetoothDevice;
use windows::Devices::Bluetooth::Rfcomm::{RfcommServiceId, RfcommServiceProvider};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Foundation::TypedEventHandler;
use windows::Networking::Sockets::{
    SocketProtectionLevel, StreamSocket, StreamSocketListener,
    StreamSocketListenerConnectionReceivedEventArgs,
};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Service UUID of PROTOCOL.md section 2.
const SERVICE_UUID: GUID = GUID::from_u128(0x7b3c2a10_6f1e_4b8e_9d1a_5c0e2f7a4b61);
const PROTECTION: SocketProtectionLevel =
    SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication;

fn init_apartment() {
    // SAFETY: initialising COM for the current thread; a repeated call returns a benign error.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn winrt_err(e: &windows::core::Error) -> io::Error {
    io::Error::other(format!("bluetooth: {e}"))
}

/// Turn a connected WinRT socket into a stream the engine can use.
fn wrap_socket(socket: StreamSocket) -> io::Result<BoxedStream> {
    // WinRT objects are apartment-bound; agile references let helper threads use them.
    let input = AgileReference::new(&socket.InputStream().map_err(|e| winrt_err(&e))?)
        .map_err(|e| winrt_err(&e))?;
    let output = AgileReference::new(&socket.OutputStream().map_err(|e| winrt_err(&e))?)
        .map_err(|e| winrt_err(&e))?;
    let socket = AgileReference::new(&socket).map_err(|e| winrt_err(&e))?;
    let (engine_side, bridge_side) = tokio::io::duplex(64 * 1024);
    let (mut bridge_read, mut bridge_write) = tokio::io::split(bridge_side);

    // socket -> engine
    let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
    std::thread::spawn(move || {
        init_apartment();
        let Ok(input) = input.resolve() else {
            return;
        };
        let Ok(reader) = DataReader::CreateDataReader(&input) else {
            return;
        };
        let _ = reader.SetInputStreamOptions(InputStreamOptions::Partial);
        loop {
            let Ok(op) = reader.LoadAsync(4096) else {
                break;
            };
            let Ok(n) = op.join() else { break };
            if n == 0 {
                break;
            }
            let mut buf = vec![0u8; n as usize];
            if reader.ReadBytes(&mut buf).is_err() || in_tx.blocking_send(buf).is_err() {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(chunk) = in_rx.recv().await {
            if bridge_write.write_all(&chunk).await.is_err() {
                break;
            }
        }
        let _ = bridge_write.shutdown().await;
    });

    // engine -> socket
    let (out_tx, out_rx) = std_mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        init_apartment();
        let Ok(output) = output.resolve() else {
            return;
        };
        let Ok(writer) = DataWriter::CreateDataWriter(&output) else {
            return;
        };
        while let Ok(chunk) = out_rx.recv() {
            if writer.WriteBytes(&chunk).is_err() {
                break;
            }
            let ok = writer.StoreAsync().and_then(|op| op.join()).is_ok();
            if !ok {
                break;
            }
        }
        // Dropping the socket (held by the reader thread's closure owner below) ends the link.
    });
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            match bridge_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        drop(out_tx);
        // Closing the socket unblocks the reader thread.
        if let Ok(socket) = socket.resolve() {
            let _ = socket.Close();
        }
    });
    Ok(Box::new(engine_side))
}

/// Accepts connections from paired devices and hands them to the manager.
pub struct RfcommServer {
    _provider: RfcommServiceProvider,
    _listener: StreamSocketListener,
}

impl RfcommServer {
    /// Register the service and start advertising. Fails when there is no Bluetooth adapter.
    pub fn start(mgr: Arc<PeerManager>, rt: tokio::runtime::Handle) -> io::Result<Self> {
        init_apartment();
        let id = RfcommServiceId::FromUuid(SERVICE_UUID).map_err(|e| winrt_err(&e))?;
        let provider = RfcommServiceProvider::CreateAsync(&id)
            .and_then(|op| op.join())
            .map_err(|e| winrt_err(&e))?;
        let listener = StreamSocketListener::new().map_err(|e| winrt_err(&e))?;
        listener
            .ConnectionReceived(&TypedEventHandler::new(
                move |_,
                      args: windows::core::Ref<
                    '_,
                    StreamSocketListenerConnectionReceivedEventArgs,
                >| {
                    if let Some(args) = args.as_ref() {
                        if let Ok(socket) = args.Socket() {
                            let mgr = Arc::clone(&mgr);
                            let rt = rt.clone();
                            let _guard = rt.enter();
                            if let Ok(stream) = wrap_socket(socket) {
                                mgr.accept_stream(stream);
                            }
                        }
                    }
                    Ok(())
                },
            ))
            .map_err(|e| winrt_err(&e))?;
        let service_id = provider.ServiceId().map_err(|e| winrt_err(&e))?;
        listener
            .BindServiceNameWithProtectionLevelAsync(
                &service_id.AsString().map_err(|e| winrt_err(&e))?,
                PROTECTION,
            )
            .and_then(|op| op.join())
            .map_err(|e| winrt_err(&e))?;
        provider
            .StartAdvertising(&listener)
            .map_err(|e| winrt_err(&e))?;
        tracing::info!("bluetooth service advertised");
        Ok(Self {
            _provider: provider,
            _listener: listener,
        })
    }
}

/// Connects to paired devices that run TransClip. The peer's Bluetooth address is not known, so
/// every paired device is tried; the handshake then checks who answered.
pub struct WinBtDialer {
    rt: tokio::runtime::Handle,
}

impl WinBtDialer {
    /// Create a dialer that spawns its blocking work on `rt`.
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        Self { rt }
    }
}

/// Device that answered last time; tried first so the usual phone connects in one step.
static LAST_GOOD: std::sync::Mutex<Option<HSTRING>> = std::sync::Mutex::new(None);

fn try_connect_blocking() -> io::Result<StreamSocket> {
    init_apartment();
    let selector =
        BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(|e| winrt_err(&e))?;
    let devices = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .and_then(|op| op.join())
        .map_err(|e| winrt_err(&e))?;
    let service = RfcommServiceId::FromUuid(SERVICE_UUID).map_err(|e| winrt_err(&e))?;
    let mut last = io::Error::other("no paired Bluetooth devices");
    let mut ordered: Vec<DeviceInformation> = devices.into_iter().collect();
    let preferred = LAST_GOOD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(pref) = preferred {
        ordered.sort_by_key(|d| d.Id().map_or(true, |id| id != pref));
    }
    for info in &ordered {
        let attempt = (|| -> windows::core::Result<StreamSocket> {
            let id: HSTRING = info.Id()?;
            let device = BluetoothDevice::FromIdAsync(&id)?.join()?;
            let services = device
                .GetRfcommServicesForIdAsync(&service)?
                .join()?
                .Services()?;
            let first = services.GetAt(0)?;
            let socket = StreamSocket::new()?;
            socket
                .ConnectWithProtectionLevelAsync(
                    &first.ConnectionHostName()?,
                    &first.ConnectionServiceName()?,
                    PROTECTION,
                )?
                .join()?;
            Ok(socket)
        })();
        match attempt {
            Ok(socket) => {
                if let Ok(id) = info.Id() {
                    *LAST_GOOD
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id);
                }
                return Ok(socket);
            }
            Err(e) => last = winrt_err(&e),
        }
    }
    Err(last)
}

impl BtDialer for WinBtDialer {
    fn connect(&self, _peer: DeviceId) -> BtConnectFuture {
        let rt = self.rt.clone();
        Box::pin(async move {
            let socket = rt
                .spawn_blocking(try_connect_blocking)
                .await
                .map_err(io::Error::other)?
                .inspect_err(|e| tracing::debug!(error = %e, "bluetooth connect failed"))?;
            tracing::debug!("bluetooth connected");
            wrap_socket(socket)
        })
    }
}

/// Whether the types we need exist on this system (used to decide if Bluetooth is offered).
pub fn available() -> bool {
    RfcommServiceId::FromUuid(SERVICE_UUID).is_ok()
}
