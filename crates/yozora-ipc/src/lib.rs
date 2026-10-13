//! Control sockets between Yozora apps.
//!
//! Each app listens on `$XDG_RUNTIME_DIR/yozora/<id>.sock`. A client connects, writes one JSON
//! request on one line, reads one JSON response line, and disconnects.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

use async_io::{Async, Timer};
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, FutureExt, io::BufReader as AsyncBufReader};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl Request {
    pub fn new(cmd: impl Into<String>, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { cmd: cmd.into(), args: args.into_iter().map(Into::into).collect() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Self { ok: true, data: serde_json::Value::Null, error: None }
    }

    pub fn data(data: serde_json::Value) -> Self {
        Self { ok: true, data, error: None }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { ok: false, data: serde_json::Value::Null, error: Some(message.into()) }
    }
}

/// A request waiting for the app to answer it.
pub struct Incoming {
    pub request: Request,
    reply: async_channel::Sender<Response>,
}

impl Incoming {
    pub fn reply(self, response: Response) {
        let _ = self.reply.try_send(response);
    }
}

/// A bound socket. Removes the socket file when dropped.
pub struct Server {
    listener: Async<UnixListener>,
    path: PathBuf,
}

/// How long a client may take to send its request.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the app may take to answer, which covers benchmarks that animate for a while.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

impl Server {
    /// Binds `path`. Fails if another live process already answers there; a stale socket left by
    /// a crash is replaced.
    pub fn bind(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            if is_alive(path) {
                anyhow::bail!("already running (socket {})", path.display());
            }
            std::fs::remove_file(path)?;
        }
        let listener = Async::new(UnixListener::bind(path)?)?;
        Ok(Self { listener, path: path.to_path_buf() })
    }

    /// Accepts connections forever, handing each request to `sink` and writing back the reply.
    /// Each connection gets its own thread, so a slow reply (a benchmark) doesn't block others.
    pub async fn run(self, sink: async_channel::Sender<Incoming>) {
        loop {
            let stream = match self.listener.accept().await {
                Ok((stream, _)) => stream,
                Err(e) => {
                    tracing::warn!("control socket accept failed: {e}");
                    Timer::after(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let client_sink = sink.clone();
            let spawned = std::thread::Builder::new().name("yz-ipc-client".into()).spawn(move || {
                if let Err(e) = async_io::block_on(handle(stream, &client_sink)) {
                    tracing::debug!("control socket client: {e:#}");
                }
            });
            if let Err(e) = spawned {
                tracing::warn!("control socket: {e}");
            }
            if sink.is_closed() {
                break;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn handle(stream: Async<UnixStream>, sink: &async_channel::Sender<Incoming>) -> anyhow::Result<()> {
    let mut reader = AsyncBufReader::new(&stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .or(async {
            Timer::after(CLIENT_TIMEOUT).await;
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "client sent nothing"))
        })
        .await?;
    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => {
            let (reply, answer) = async_channel::bounded(1);
            sink.send(Incoming { request, reply }).await?;
            async { answer.recv().await.unwrap_or_else(|_| Response::error("app is shutting down")) }
                .or(async {
                    Timer::after(REPLY_TIMEOUT).await;
                    Response::error("the app took too long to answer")
                })
                .await
        }
        Err(e) => Response::error(format!("bad request: {e}")),
    };
    let mut out = serde_json::to_string(&response)?;
    out.push('\n');
    (&stream).write_all(out.as_bytes()).await?;
    Ok(())
}

/// Sends one request and waits for the reply. Blocking, with a timeout.
pub fn request(path: &Path, request: &Request) -> anyhow::Result<Response> {
    // A little longer than the server allows, so its timeout error arrives.
    request_timeout(path, request, REPLY_TIMEOUT + Duration::from_secs(5))
}

/// `request` with a custom reply timeout.
pub fn request_timeout(path: &Path, request: &Request, timeout: Duration) -> anyhow::Result<Response> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    Ok(serde_json::from_str(reply.trim())?)
}

/// Whether something answers `ping` at `path`.
pub fn is_alive(path: &Path) -> bool {
    request_timeout(path, &Request::new("ping", [] as [&str; 0]), Duration::from_secs(1)).is_ok_and(|r| r.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_single_instance() {
        let path = std::env::temp_dir().join(format!("yozora-ipc-{}/t.sock", std::process::id()));
        let server = Server::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || async_io::block_on(server.run(tx)));
        std::thread::spawn(move || {
            while let Ok(incoming) = rx.recv_blocking() {
                let reply = match incoming.request.cmd.as_str() {
                    "ping" => Response::ok(),
                    "echo" => Response::data(serde_json::json!(incoming.request.args)),
                    _ => Response::error("unknown"),
                };
                incoming.reply(reply);
            }
        });
        let echoed = request(&path, &Request::new("echo", ["a", "b"])).unwrap();
        assert_eq!(echoed.data, serde_json::json!(["a", "b"]));
        assert!(!request(&path, &Request::new("nope", [] as [&str; 0])).unwrap().ok);
        assert!(Server::bind(&path).is_err(), "second instance must be refused");
    }
}
