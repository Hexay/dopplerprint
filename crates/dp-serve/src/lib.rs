//! `dopplerprint serve`: a local drag-and-drop web app in front of a [`LocateBackend`].

mod assets;
mod backend;
mod fake;
mod http;
mod jobs;
mod upload;

pub use backend::*;
pub use fake::FakeBackend;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(clap::Args, Clone, Debug)]
pub struct ServeArgs {
    /// Address to listen on. Keep the default unless you know you want other devices to reach it.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port; if taken, the next free one of the following ten is used. 0 picks any free port.
    #[arg(long, default_value_t = 8737)]
    pub port: u16,
    /// Open the page in the default browser once the server is up.
    #[arg(long)]
    pub open: bool,
    /// Where uploads, progress and results are kept [default: <cache>/dopplerprint/jobs].
    #[arg(long)]
    pub jobs: Option<PathBuf>,
    /// Largest accepted upload, MB.
    #[arg(long, default_value_t = 1024)]
    pub max_upload_mb: u64,
}

pub struct Handle {
    pub addr: SocketAddr,
    server: Arc<tiny_http::Server>,
    accept: JoinHandle<()>,
}

impl Handle {
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn stop(self) {
        self.server.unblock();
        let _ = self.accept.join();
    }

    pub fn wait(self) {
        let _ = self.accept.join();
    }
}

/// `$XDG_CACHE_HOME/dopplerprint`, else `~/.cache/dopplerprint`, else `%LOCALAPPDATA%\dopplerprint`.
pub fn default_cache_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    env("XDG_CACHE_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".cache")))
        .or_else(|| env("LOCALAPPDATA"))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dopplerprint")
}

/// Binds, recovers persisted jobs, starts the worker and the accept loop; returns immediately.
pub fn start(args: &ServeArgs, backend: Box<dyn LocateBackend>) -> Result<Handle> {
    let jobs_dir = args.jobs.clone().unwrap_or_else(|| default_cache_dir().join("jobs"));
    let store = Arc::new(jobs::Store::open(&jobs_dir)?);
    let queue = Arc::new(jobs::Queue::default());
    for id in store.recover()? {
        queue.push(id);
    }
    let (server, addr) = bind(&args.host, args.port)?;
    let server = Arc::new(server);
    jobs::spawn_worker(store.clone(), queue.clone(), Arc::from(backend));

    let loopback = addr.ip().is_loopback();
    let hosts = (!addr.ip().is_unspecified()).then(|| {
        let p = addr.port();
        let mut hs = vec![format!("{}:{p}", host_literal(&addr))];
        if loopback {
            hs.extend([format!("localhost:{p}"), format!("127.0.0.1:{p}"), format!("[::1]:{p}")]);
        }
        hs
    });
    let ctx = Arc::new(http::Ctx { store, queue, max_upload: args.max_upload_mb.saturating_mul(1024 * 1024), hosts });
    let srv = server.clone();
    let accept = std::thread::Builder::new().name("dp-serve-accept".into()).spawn(move || {
        for req in srv.incoming_requests() {
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name("dp-serve-req".into()).spawn(move || http::handle(&ctx, req));
        }
    })?;
    Ok(Handle { addr, server, accept })
}

/// Serves until the process is killed.
pub fn run(args: ServeArgs, backend: Box<dyn LocateBackend>) -> Result<()> {
    let handle = start(&args, backend)?;
    let url = handle.url();
    println!("dopplerprint is running at {url}  (Ctrl+C to stop)");
    if !handle.addr.ip().is_loopback() {
        println!("Warning: listening beyond this computer; anyone who can reach {url} can upload and see results.");
    }
    if args.open {
        open_browser(&url);
    }
    handle.wait();
    Ok(())
}

fn bind(host: &str, port: u16) -> Result<(tiny_http::Server, SocketAddr)> {
    let tries = if port == 0 { 1 } else { 11 };
    let mut last = None;
    for p in (0..tries).map(|i| port.saturating_add(i)) {
        match tiny_http::Server::http((host, p)) {
            Ok(s) => {
                let addr = s.server_addr().to_ip().context("server has no IP address")?;
                return Ok((s, addr));
            }
            Err(e) => last = Some(e),
        }
    }
    Err(anyhow::anyhow!("{}", last.map(|e| e.to_string()).unwrap_or_default()))
        .with_context(|| format!("couldn't listen on {host}:{port} (or the next {} ports)", tries - 1))
}

fn host_literal(addr: &SocketAddr) -> String {
    match addr {
        SocketAddr::V4(a) => a.ip().to_string(),
        SocketAddr::V6(a) => format!("[{}]", a.ip()),
    }
}

fn open_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    if cmd.arg(url).spawn().is_err() {
        println!("Couldn't open a browser; visit {url} yourself.");
    }
}
