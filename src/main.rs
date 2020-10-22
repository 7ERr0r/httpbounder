use crate::marker::BcData;
use crate::marker::BcDataMarked;
use actix_web::body::Body;
use actix_web::body::BodySize;
use actix_web::body::MessageBody;
use actix_web::http::HeaderName;
use actix_web::http::HeaderValue;
use actix_web::http::StatusCode;
use actix_web::web::Bytes;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;
use tokio::sync::mpsc::Sender;

use reqwest::header::ACCEPT;
use tokio::sync::Mutex;

use actix_web::Error;
use actix_web::HttpRequest;
use actix_web::HttpResponse;

use actix_web::{web, App, HttpServer};

use std::sync::Arc;

pub mod marker;
pub mod mjpeg_marker;

use crate::mjpeg_marker::MJPEGStartMarker;

const USER_AGENT: &str = "Httpbounder (Rust/reqwest)";


async fn forward(
    _req: HttpRequest,
    bc_mutex: web::Data<Arc<Mutex<BroadcastChannel>>>,
) -> Result<HttpResponse, Error> {
    let mut bc = bc_mutex.lock().await;
    let mut client_resp = HttpResponse::build(bc.status);
    let headers = &bc.headers;

    for (header_name, header_value) in headers {
        client_resp.header(header_name.clone(), header_value.clone());
    }
    //println!("{:?}", &res.headers());

    let (tx, rx) = tokio::sync::mpsc::channel::<BcData>(256);
    bc.tx_vec.push(BroadcastSender {
        tx,
        header_sent: false,
    });

    drop(bc);
    let msg = Box::new(ReceiverBodyStream { rx });
    Ok(client_resp.body(Body::Message(msg)))
}

struct ReceiverBodyStream {
    rx: tokio::sync::mpsc::Receiver<BcData>,
}

impl MessageBody for ReceiverBodyStream {
    fn size(&self) -> BodySize {
        BodySize::Stream
    }

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Error>>> {
        use futures::prelude::*;
        let fut = self.rx.recv();

        let mut fut = Box::pin(fut);
        let fut = Pin::as_mut(&mut fut);

        // Context passed for waking
        match fut.poll(cx) {
            Poll::Ready(Some(x)) => {
                //let v: Vec<u8> = (Arc::make_mut(&mut x).clone()).into();
                Poll::Ready(Some(Ok(x)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct BroadcastSender {
    pub tx: Sender<BcData>,
    pub header_sent: bool,
}

struct BroadcastChannel {
    tx_vec: Vec<BroadcastSender>,
    status: StatusCode,
    headers: Vec<(HeaderName, HeaderValue)>,
}
impl BroadcastChannel {
    fn new() -> Self {
        Self {
            tx_vec: Vec::new(),
            status: actix_web::http::StatusCode::BAD_GATEWAY,
            headers: Vec::new(),
        }
    }
}

struct ClientSession {
    sconfig: SourceClientConfig,
    client: reqwest::Client,
    channels: Arc<Mutex<BroadcastChannel>>,
}

impl ClientSession {
    fn new(sconfig: SourceClientConfig, channels: Arc<Mutex<BroadcastChannel>>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("should be able to build reqwest client");

        Self {
            sconfig,
            client,
            channels,
        }
    }

    async fn broadcast(&mut self, b: &BcDataMarked) {
        let mut bc = self.channels.lock().await;

        let v = &mut bc.tx_vec;

        let mut i = 0;
        while i < v.len() {
            let tx = &mut v[i];

            if b.valid_start {
                tx.header_sent = true;
            }

            if tx.header_sent {
                let sent = tx.tx.try_send(b.bytes.clone());
                match sent {
                    Ok(_) => {}
                    Err(_err) => {
                        //println!("not sent {}", err);
                        let l = v.len();
                        v.swap(i, l - 1);
                        v.pop();
                    }
                }
            }
            i += 1;
        }
    }

    async fn run(&mut self) -> Result<(), anyhow::Error> {
        let client = &self.client;

        let url = reqwest::Url::parse(&self.sconfig.url)?;
        println!("GET {}", url);
        let mut req = reqwest::Request::new(reqwest::Method::GET, url);
        let h = req.headers_mut();
        h.insert(ACCEPT, "*/*".parse().unwrap());

        match &self.sconfig.user {
            None => {}
            Some(user) => {
                let b64credentials = base64::encode(user);
                h.insert(
                    "authorization",
                    format!("Basic {}", b64credentials).parse().unwrap(),
                );
            }
        }
        let mut res = client.execute(req).await?;
        println!("Status: {}", res.status());

        let mut mjpegm = MJPEGStartMarker::new();
        mjpegm.read_headers(res.headers());

        {
            let mut bc = self.channels.lock().await;
            bc.status = res.status();

            bc.headers.clear();
            for (header_name, header_value) in res
                .headers()
                .iter()
                .filter(|(h, _)| *h != "connection" && *h != "content-length")
            {
                //println!("header: {} {:?}", header_name, header_value);
                bc.headers.push((header_name.clone(), header_value.clone()));
            }
        }

        while let Some(chunk) = res.chunk().await? {
            //println!("len: {}", chunk.len());
            //println!("chunk: {:?}", chunk);

            for marked in mjpegm.mark_chunk(&chunk).iter().filter_map(|x| x.as_ref()) {
                if marked.valid_start {
                    //println!("__valid: {:?}", marked.bytes);
                } else {
                    //println!("invalid: {:?}", marked.bytes);
                }
                self.broadcast(marked).await;
            }
        }

        Ok(())
    }
}
async fn fetcher(sconfig: SourceClientConfig, bc: Arc<Mutex<BroadcastChannel>>) {
    let mut sess = ClientSession::new(sconfig, bc.clone());
    loop {
        let res = sess.run().await;

        match res {
            Err(err) => println!("fetcher: ClientSession.run: {}", err),
            _ => {}
        }

        let sleep_duration = std::time::Duration::from_millis(3000);
        tokio::time::delay_for(sleep_duration).await;
    }
}
struct SourceClientConfig {
    user: Option<String>,
    url: String,
}

async fn run() -> Result<(), anyhow::Error> {
    let matches = clap::App::new("httpbounder")
        .version("0.1.0")
        .author("Szperak")
        .about("Broadcast http streams such as mjpeg. Auto header detection/sync")
        .arg(
            clap::Arg::with_name("user")
                .short("u")
                .long("user")
                .takes_value(true)
                .help("example: 'user:password'"),
        )
        .arg(
            clap::Arg::with_name("input")
                .short("i")
                .long("input")
                .takes_value(true)
                .required(true)
                .help("http stream URL, eg. http://1.2.3.4/mjpg/video.mjpg"),
        )
        .arg(
            clap::Arg::with_name("output")
                .short("o")
                .long("output")
                .takes_value(true)
                .default_value("/video.mjpg")
                .help("http stream path, eg. /video.mjpg"),
        )
        .arg(
            clap::Arg::with_name("bind")
                .short("b")
                .long("bind")
                .takes_value(true)
                .default_value("0.0.0.0:8080")
                .help("actix HttpServer bind addr"),
        )
        .get_matches();

    let stream_link = matches
        .value_of("input")
        .ok_or(HttpBounderError::UrlNotProvided)?;

    let user: Option<String> = matches.value_of("user").map(|v| v.into());

    let output_path = matches.value_of("output").unwrap().to_string();

    let bind_addr = matches.value_of("bind").unwrap().to_string();

    let sconfig = SourceClientConfig {
        user,
        url: stream_link.to_string(),
    };

    // #[cfg(target_os = "linux")]
    // let guard = pprof::ProfilerGuard::new(100).unwrap();

    let channels = Arc::new(Mutex::new(BroadcastChannel::new()));

    let channels_clone = channels.clone();
    actix_rt::Arbiter::spawn(async {
        fetcher(sconfig, channels_clone).await;
    });

    // #[cfg(target_os = "linux")]
    // actix_rt::Arbiter::spawn(async move {
    //     let sleep_duration = std::time::Duration::from_millis(30000);
    //     tokio::time::delay_for(sleep_duration).await;
    //     if let Ok(report) = guard.report().build() {
    //         use std::fs::File;
    //         use std::io::prelude::*;
    //         let file = File::create("flamegraph.svg").unwrap();
    //         report.flamegraph(file).unwrap();
    //     };
    // });

    let channels = Box::new(channels);

    HttpServer::new(move || {
        let channels = *channels.clone();
        let output_path = output_path.clone();

        App::new()
            .data(channels)
            .service(web::resource(output_path).route(web::route().to(forward)))
    })
    .bind(bind_addr)?
    .run()
    .await?;

    Ok(())
}
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let result = run().await;
    match result {
        Err(err) => {
            println!("err: {}", err);
        }
        Ok(_) => {}
    }

    Ok(())
}

#[derive(Debug, derive_more::Display)]
pub enum HttpBounderError {
    #[display(fmt = "UrlNotProvided: --url")]
    UrlNotProvided,
}
impl std::error::Error for HttpBounderError {}
