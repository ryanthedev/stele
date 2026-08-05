//! The network seam: one trait, one set of bounds, and one real
//! implementation that is the only code in this workspace that opens a socket.
//!
//! Everything above this file — [`crate::media::remote`]'s URL rewriting, the
//! cache, the failure-to-alt-text rule — is pure and testable against a fake.
//! That split is the whole design: the suite must never touch the network, and
//! the way to guarantee that is for the network to be a parameter rather than
//! an ambient capability.
//!
//! ## Redirects are the caller's problem on purpose
//!
//! A [`Fetcher`] returns [`Fetched::Redirect`] instead of following the hop
//! itself, and ureq is configured (`max_redirects(0)`) to hand the 3xx back
//! rather than chase it. That looks like extra work — the client already knows
//! how — and it buys two things a delegated redirect cannot:
//!
//! - **The cap is testable.** A redirect loop, and the exact hop the cap bites
//!   on, are exercised by [`crate::media::remote`]'s tests against a fake that
//!   never opens a socket. A cap enforced inside ureq could only be tested by
//!   standing up a server that redirects to itself.
//! - **The scheme check applies to every hop, not just the first.** A URL that
//!   starts `https://` and redirects to `file:///etc/passwd` is exactly the
//!   shape this viewer must refuse, and refusing it means re-running the same
//!   check on each `Location` — see `remote::resolve`.

use std::fmt;
use std::time::Duration;

/// What one HTTP request produced, when it produced anything at all.
///
/// Deliberately not `http::Response`: the seam's whole point is that a fake
/// can implement it in five lines, and a fake that has to build a real
/// response type is a fake nobody writes correctly.
#[derive(Debug)]
pub enum Fetched {
    /// A 2xx with its body, already bounded by
    /// [`FetchLimits::max_response_bytes`].
    Body(Vec<u8>),
    /// A 3xx with its `Location` header, verbatim — absolute or relative, not
    /// yet resolved against the request URL and not yet scheme-checked. Both
    /// are `remote::resolve`'s job, for the reason the module doc gives.
    Redirect(String),
}

/// Every bound a fetch answers to. One struct rather than five constants,
/// because a test that wants a 20 ms budget or an 8-byte ceiling must be able
/// to say so without waiting out the production value.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    /// Longest a connection may take to open, DNS and TLS handshake included.
    pub connect: Duration,
    /// Longest one request may take end to end, connect included. This is the
    /// bound that stops a host trickling one byte a second from holding the
    /// viewer open.
    pub request: Duration,
    /// Longest **all** of a document's fetches together may take. Once it is
    /// spent, every image still unresolved falls back to its alt text
    /// immediately.
    ///
    /// [`FetchLimits::request`] alone does not bound a document: a page with
    /// forty remote images pointed at a slow host costs forty times the
    /// per-request timeout, and the reader sees nothing at all for all of it.
    /// This is what makes the worst case a property of the flag rather than of
    /// the document.
    ///
    /// **It is checked before each request, not during one**, so the true
    /// ceiling is `document_budget + request` — a fetch that starts a
    /// millisecond inside the budget still gets its full per-request timeout.
    /// At the production values that is 30 s, not 20 s. Tightening it would
    /// mean recomputing the client's timeout per call from the budget
    /// remaining, which ureq's agent-level configuration does not do; the
    /// number is stated here rather than the tighter one implied.
    pub document_budget: Duration,
    /// Most bytes a single response body may be, before it is a refusal.
    pub max_response_bytes: u64,
    /// Most `Location` hops one URL may take before it is a refusal. Zero
    /// means "no redirect at all is followed".
    pub max_redirects: u8,
}

impl Default for FetchLimits {
    /// The production bounds. Every one of them is a *stated* number rather
    /// than a derived one except the byte ceiling, which is deliberately not
    /// ours to pick.
    fn default() -> Self {
        FetchLimits {
            // Enough for a TLS handshake to a slow host on a poor link;
            // short enough that a black-holed address is not mistaken for a
            // hung viewer.
            connect: Duration::from_secs(5),
            // Twice the connect budget, so a connection that opens has real
            // time to deliver a diagram before it is given up on.
            request: Duration::from_secs(10),
            // Two full request timeouts. A document whose images are all
            // dead costs the reader twenty seconds *once*, not ten seconds
            // per image — and a document whose host is merely slow still gets
            // two images through before the rest degrade.
            document_budget: Duration::from_secs(20),
            // **Not invented here.** `gfx::Limits::max_alloc` is the ceiling
            // the decoder already applies to the RGBA8 buffer this download
            // will become, so a body past it cannot produce a drawable image
            // however the bytes are arranged — which makes it the honest place
            // to stop reading. Picking a smaller round number would be a
            // second, unexplained policy sitting in front of the first.
            //
            // Say the size out loud, because it is large: 256 MiB. It is not
            // the operative bound in practice and is not meant to be —
            // `request` is. A host would have to sustain ~25 MiB/s for the
            // full ten seconds to reach this ceiling at all; anything slower
            // hits the clock first. The pair is what bounds memory, not either
            // one alone.
            max_response_bytes: gfx::Limits::default().max_alloc,
            // Enough for the ordinary shapes — http→https, a CDN's
            // canonical-host bounce, a shortener — and far short of a loop.
            max_redirects: 4,
        }
    }
}

/// Why a remote image did not become a local file.
///
/// Hand-rolled with a manual [`fmt::Display`] per `docs/code-standards.md:69`
/// (`thiserror` is `crates/probe`-only). Variants carry what a caller needs to
/// decide rather than a pre-formatted string — the cap that was hit, the
/// status that came back — even though today every caller does the same thing
/// with all of them: leaves the image as its alt text. They are distinct
/// because the *tests* discriminate on them, and a test that can only assert
/// "something failed" cannot tell a size refusal from a redirect loop.
#[derive(Debug)]
pub enum FetchError {
    /// A deadline passed: [`FetchLimits::connect`] or [`FetchLimits::request`].
    Timeout,
    /// The response body passed [`FetchLimits::max_response_bytes`].
    TooLarge { limit: u64 },
    /// A final status that was neither 2xx nor a redirect.
    Status(u16),
    /// DNS, TLS, a dropped connection, a malformed response — anything the
    /// client itself refused. The string is the client's own words; it reaches
    /// no user interface, only a test assertion and a debug print.
    Transport(String),
    /// The URL's scheme is not `http` or `https`. Carries the scheme as
    /// written, which is the whole point of the variant: `file`, `data` and
    /// `javascript` are the ones worth being able to name in a test.
    UnsupportedScheme(String),
    /// More `Location` hops than [`FetchLimits::max_redirects`] allows —
    /// a loop, or a chain too long to be anything but one.
    TooManyRedirects { cap: u8 },
    /// A 3xx whose `Location` is missing, empty, or not resolvable to an
    /// absolute `http`/`https` URL.
    UnusableRedirect,
    /// The bytes arrived and `gfx::decode` would not have them: not an image
    /// in any format this viewer draws, or one whose header claims dimensions
    /// past `gfx::Limits`.
    NotAnImage,
    /// The bytes arrived and were good, and the cache could not be written.
    /// Distinct from [`FetchError::NotAnImage`] because it is *our* fault, not
    /// the host's, and a reader debugging a stubbornly-alt-text image should
    /// be able to tell a full disk from a bad server.
    CacheWrite(std::io::Error),
    /// [`FetchLimits::document_budget`] was already spent when this image came
    /// up. No request was made.
    BudgetSpent,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Timeout => write!(f, "the host did not answer in time"),
            FetchError::TooLarge { limit } => write!(
                f,
                "the response is larger than the {} MiB stele will download",
                limit / (1024 * 1024)
            ),
            FetchError::Status(code) => write!(f, "the host answered {code}"),
            FetchError::Transport(reason) => write!(f, "could not reach the host: {reason}"),
            FetchError::UnsupportedScheme(scheme) => {
                write!(f, "{scheme} is not a scheme stele will fetch over")
            }
            FetchError::TooManyRedirects { cap } => {
                write!(f, "more than {cap} redirects")
            }
            FetchError::UnusableRedirect => {
                write!(f, "a redirect with no usable http or https target")
            }
            FetchError::NotAnImage => write!(f, "the bytes are not an image stele can draw"),
            FetchError::CacheWrite(err) => write!(f, "could not write the image cache: {err}"),
            FetchError::BudgetSpent => {
                write!(f, "this document's fetch budget was already spent")
            }
        }
    }
}

impl std::error::Error for FetchError {}

/// One HTTP request, bounded by `limits`.
///
/// The only method that may touch a socket in this workspace. Implementors
/// must not follow redirects — see the module doc.
///
/// Takes `&self` rather than `&mut self`, and is `Send + Sync`, because the
/// process holds exactly one of these behind a `&'static`
/// (`remote::production`) and a `&'static` to a non-`Sync` value cannot be
/// had. An implementation that wants to count its own calls therefore needs a
/// [`std::sync::Mutex`] rather than a `Cell` — which the fakes in
/// `crate::media::remote`'s tests do, and which is the whole cost of the
/// bound. Nothing here is ever called from two threads: stele's load path is
/// single-threaded and this is a lifetime requirement, not a concurrency one.
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &str, limits: &FetchLimits) -> Result<Fetched, FetchError>;
}

/// A [`Fetcher`] that never fetches: every call is
/// [`FetchError::UnsupportedScheme`]`("<none>")`.
///
/// Not a test double — `crate::media::remote` has its own. This is what a
/// build compiled **without** the `remote-images` feature would use if
/// something managed to construct a policy object anyway, so that "the network
/// stack is not compiled in" and "the network stack is compiled in and
/// disabled" cannot diverge in behavior.
#[derive(Debug, Default)]
pub struct NoFetcher;

impl Fetcher for NoFetcher {
    fn fetch(&self, _url: &str, _limits: &FetchLimits) -> Result<Fetched, FetchError> {
        Err(FetchError::UnsupportedScheme("<no network>".to_string()))
    }
}

/// The real one: ureq over rustls, configured from [`FetchLimits`].
///
/// Compiled only under the `remote-images` feature. See `Cargo.toml` for why
/// rustls and what the dependency costs.
#[cfg(feature = "remote-images")]
#[derive(Debug)]
pub struct HttpFetcher {
    agent: ureq::Agent,
}

#[cfg(feature = "remote-images")]
impl HttpFetcher {
    /// Builds an agent whose every timeout comes from `limits`.
    ///
    /// Four settings here are load-bearing rather than taste:
    ///
    /// - `max_redirects(0)` + `max_redirects_will_error(false)` make ureq hand
    ///   the 3xx response back instead of chasing it or erroring. That is what
    ///   puts the hop count and the per-hop scheme check in our own code,
    ///   where a fake can reach them.
    /// - `http_status_as_error(false)` makes a 404 an ordinary response rather
    ///   than a client error, so [`Fetcher::fetch`] decides what every status
    ///   means in one place instead of splitting the decision across a
    ///   `Result` boundary.
    /// - `timeout_global` covers DNS through the last body byte, so it bounds
    ///   the slow-trickle case that a connect timeout alone does not.
    /// - A named `user_agent`, because a host that dislikes being scraped
    ///   deserves to be able to say so to the right client.
    pub fn new(limits: &FetchLimits) -> Self {
        let config = ureq::config::Config::builder()
            .timeout_connect(Some(limits.connect))
            .timeout_global(Some(limits.request))
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .user_agent(concat!("stele/", env!("CARGO_PKG_VERSION")))
            .build();
        HttpFetcher {
            agent: config.new_agent(),
        }
    }
}

#[cfg(feature = "remote-images")]
impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str, limits: &FetchLimits) -> Result<Fetched, FetchError> {
        let mut response = self.agent.get(url).call().map_err(transport_error)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get(ureq::http::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            return Ok(Fetched::Redirect(location));
        }
        if !(200..300).contains(&status) {
            return Err(FetchError::Status(status));
        }
        // `.limit(..)` before `read_to_vec`, never after: ureq's unconfigured
        // `read_to_vec` has its own default ceiling, and relying on someone
        // else's default for the one bound that stops a hostile host eating
        // memory is exactly the kind of borrowed invariant that goes stale in
        // a minor version bump.
        let bytes = response
            .body_mut()
            .with_config()
            .limit(limits.max_response_bytes)
            .read_to_vec()
            .map_err(transport_error)?;
        Ok(Fetched::Body(bytes))
    }
}

/// Maps ureq's error taxonomy onto ours, keeping the three outcomes a caller
/// can distinguish — a deadline, a size refusal, everything else — and folding
/// the rest into one string.
#[cfg(feature = "remote-images")]
fn transport_error(err: ureq::Error) -> FetchError {
    match err {
        ureq::Error::Timeout(_) => FetchError::Timeout,
        ureq::Error::BodyExceedsLimit(limit) => FetchError::TooLarge { limit },
        ureq::Error::StatusCode(code) => FetchError::Status(code),
        other => FetchError::Transport(other.to_string()),
    }
}

/// The test double every test in this workspace fetches through.
///
/// It lives beside the trait rather than inside one test module because two of
/// them need it — `media::remote`'s and `loader`'s — and a second copy would
/// be a second place for "what a fake fetch does" to drift from the first. It
/// is also the enforcement point for the rule that matters most here: the only
/// [`Fetcher`] a test binary ever constructs is this one, so the suite cannot
/// reach the network even by accident.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{FetchError, FetchLimits, Fetched, Fetcher};

    /// Every URL a [`Fake`] was asked for, shared with the test that built it.
    ///
    /// `RemoteImages` owns its fetcher (`Box<dyn Fetcher>`), so a test cannot
    /// reach back into the fake after handing it over. The counter therefore
    /// lives on this side of the boundary and the fake holds a handle to it —
    /// which is what makes "zero fetches" assertable at all, since the
    /// interesting case is precisely the one where nothing downstream was ever
    /// constructed to ask.
    ///
    /// `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>` because [`Fetcher`] is
    /// `Send + Sync` — see its doc comment for why that bound exists and why
    /// it is not a statement about concurrency.
    #[derive(Clone, Default)]
    pub(crate) struct Log(Arc<Mutex<Vec<String>>>);

    impl Log {
        pub(crate) fn calls(&self) -> usize {
            self.urls().len()
        }

        pub(crate) fn urls(&self) -> Vec<String> {
            self.0
                .lock()
                .expect("no test panics while holding this lock")
                .clone()
        }
    }

    /// One scripted reply. A variant per outcome the resolver discriminates
    /// on, so a test states what happened rather than encoding it.
    #[derive(Clone)]
    pub(crate) enum Reply {
        Bytes(Vec<u8>),
        RedirectTo(String),
        Timeout,
        TooLarge,
        Status(u16),
        Transport(&'static str),
    }

    /// A [`Fetcher`] that never opens a socket, logs what it was asked for,
    /// and answers from a script whose last entry repeats forever — so a
    /// redirect loop is one entry rather than a list as long as the cap.
    pub(crate) struct Fake {
        script: Vec<Reply>,
        log: Log,
        /// How long each answer "takes". Only the budget test uses it.
        delay: Duration,
    }

    impl Fake {
        pub(crate) fn new(log: &Log, script: Vec<Reply>) -> Fake {
            Fake {
                script,
                log: log.clone(),
                delay: Duration::ZERO,
            }
        }

        pub(crate) fn serving(log: &Log, bytes: Vec<u8>) -> Fake {
            Fake::new(log, vec![Reply::Bytes(bytes)])
        }

        pub(crate) fn slow(mut self, delay: Duration) -> Fake {
            self.delay = delay;
            self
        }
    }

    impl Fetcher for Fake {
        fn fetch(&self, url: &str, _limits: &FetchLimits) -> Result<Fetched, FetchError> {
            let nth = {
                let mut log = self
                    .log
                    .0
                    .lock()
                    .expect("no test panics while holding this lock");
                log.push(url.to_string());
                log.len() - 1
            };
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            match self.script.get(nth).or_else(|| self.script.last()) {
                Some(Reply::Bytes(bytes)) => Ok(Fetched::Body(bytes.clone())),
                Some(Reply::RedirectTo(to)) => Ok(Fetched::Redirect(to.clone())),
                Some(Reply::Timeout) => Err(FetchError::Timeout),
                Some(Reply::TooLarge) => Err(FetchError::TooLarge {
                    limit: FetchLimits::default().max_response_bytes,
                }),
                Some(Reply::Status(code)) => Err(FetchError::Status(*code)),
                Some(Reply::Transport(reason)) => Err(FetchError::Transport(reason.to_string())),
                None => Err(FetchError::Transport("empty script".to_string())),
            }
        }
    }

    /// A `w`×`h` PNG, for a fake to serve and for `gfx::decode` to accept.
    /// Shared for the same reason [`Fake`] is: three test modules build one.
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 100, 50, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("an in-memory PNG encode cannot fail");
        bytes
    }
}

/// A one-host HTTP server on the loopback interface, for the tests that have
/// to exercise [`HttpFetcher`] itself rather than a stand-in for it.
///
/// Everything else in the suite fetches through [`fake::Fake`], which is the
/// right double for policy questions — `remote`'s redirect ladder, the
/// document budget, the resolver — because those are about what stele *does*
/// with an answer. But four of `HttpFetcher`'s settings are claims about what
/// ureq does with a *response*, and a fake cannot check any of them: that a
/// 3xx comes back instead of being chased, that a 404 is an ordinary answer
/// and not a client error, that the body ceiling is ours and not ureq's
/// default, and that the deadline is real. Those need a real socket and a real
/// response on the other end of it.
///
/// This is not a relaxation of the no-network rule. There is no DNS, no
/// external host and no TLS here — the client is pointed at a port this
/// process opened, and every byte it reads was written four lines above the
/// assertion. What the rule protects against is a test that quietly depends on
/// something outside the machine; a listener on 127.0.0.1:0 is as inside the
/// machine as the fake is.
#[cfg(all(test, feature = "remote-images"))]
mod loopback {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// What the server does with one connection. The last entry of a script
    /// repeats forever, so "answers every request the same way" is one entry
    /// rather than a guess at how many the client will make.
    pub(super) enum Serve {
        /// Write these bytes as the whole response, then close.
        Raw(Vec<u8>),
        /// Read the request and then say nothing for this long. The client's
        /// own deadline is the only thing that ends it.
        Stall(Duration),
    }

    pub(super) struct Server {
        base: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        /// Binds an ephemeral port and starts answering from `script`.
        pub(super) fn start(script: Vec<Serve>) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").expect("loopback is bindable");
            let base = format!(
                "http://{}",
                listener
                    .local_addr()
                    .expect("a bound listener has an address")
            );
            let requests: Arc<Mutex<Vec<String>>> = Arc::default();
            let seen = Arc::clone(&requests);

            // Detached on purpose. `accept` is blocking and there is no
            // portable way to interrupt it from a `Drop`, so the alternative
            // to letting the thread die with the test binary is a shutdown
            // channel that every test would have to remember to use. The
            // thread holds one port and no other resource, and a test binary
            // is the whole lifetime in question.
            std::thread::spawn(move || {
                for (nth, stream) in listener.incoming().flatten().enumerate() {
                    let mut reader = BufReader::new(&stream);
                    let mut head = String::new();
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break,
                            Ok(_) => {
                                let blank = line == "\r\n" || line == "\n";
                                head.push_str(&line);
                                if blank {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    seen.lock()
                        .expect("no server panic holds this lock")
                        .push(head);

                    match script.get(nth).or_else(|| script.last()) {
                        Some(Serve::Raw(bytes)) => {
                            let mut stream = &stream;
                            let _ = stream.write_all(bytes);
                            let _ = stream.flush();
                        }
                        Some(Serve::Stall(how_long)) => std::thread::sleep(*how_long),
                        None => {}
                    }
                }
            });

            Server { base, requests }
        }

        /// `http://127.0.0.1:<port>/<path>`.
        pub(super) fn url(&self, path: &str) -> String {
            format!("{}/{path}", self.base)
        }

        /// The full request head of every connection served so far, in order.
        pub(super) fn requests(&self) -> Vec<String> {
            self.requests
                .lock()
                .expect("no server panic holds this lock")
                .clone()
        }
    }

    /// One HTTP/1.1 response, headers and all. Written by hand rather than by
    /// a server crate because the point of these tests is that *no* second
    /// implementation stands between the assertion and the wire.
    pub(super) fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
        for (name, value) in headers {
            out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        out.extend_from_slice(body);
        out
    }
}

#[cfg(all(test, feature = "remote-images"))]
mod http_fetcher_tests {
    use std::time::Duration;

    use super::loopback::{Serve, Server, response};
    use super::*;

    /// Production bounds are minutes-long by design; a test that waited them
    /// out would be a worse stopwatch than the one it replaced.
    fn quick() -> FetchLimits {
        FetchLimits {
            connect: Duration::from_millis(500),
            request: Duration::from_millis(500),
            ..FetchLimits::default()
        }
    }

    #[test]
    fn test_a_200_hands_back_exactly_the_bytes_the_host_sent() {
        let body = fake::png(3, 2);
        let server = Server::start(vec![Serve::Raw(response(
            "200 OK",
            &[("Content-Type", "image/png")],
            &body,
        ))]);
        let limits = quick();

        let fetched = HttpFetcher::new(&limits)
            .fetch(&server.url("a.png"), &limits)
            .expect("a 200 with a body is a successful fetch");

        match fetched {
            Fetched::Body(bytes) => {
                assert_eq!(bytes, body, "the body must survive the fetch byte for byte")
            }
            Fetched::Redirect(to) => panic!("a 200 became a redirect to {to}"),
        }
    }

    /// `max_redirects(0)` + `max_redirects_will_error(false)`, asserted by the
    /// only witness that can tell the difference: the server's own request
    /// count. A client that chased the hop would connect twice.
    #[test]
    fn test_a_redirect_comes_back_unchased_rather_than_followed_by_the_client() {
        let server = Server::start(vec![
            Serve::Raw(response(
                "302 Found",
                &[("Location", "https://elsewhere.invalid/b.png")],
                b"",
            )),
            Serve::Raw(response("200 OK", &[], b"chased it")),
        ]);
        let limits = quick();

        let fetched = HttpFetcher::new(&limits)
            .fetch(&server.url("a.png"), &limits)
            .expect("a 3xx is an answer, not a transport failure");

        match fetched {
            Fetched::Redirect(to) => assert_eq!(to, "https://elsewhere.invalid/b.png"),
            Fetched::Body(bytes) => panic!(
                "the redirect was followed: {}",
                String::from_utf8_lossy(&bytes)
            ),
        }
        assert_eq!(
            server.requests().len(),
            1,
            "the hop belongs to `remote::resolve`, so the client must make \
             exactly one request and hand the Location back"
        );
    }

    /// A 3xx with nothing to go on still reaches the resolver, which is where
    /// `UnusableRedirect` is decided. The client's job is to report, not judge.
    #[test]
    fn test_a_redirect_without_a_location_arrives_as_an_empty_target() {
        let server = Server::start(vec![Serve::Raw(response(
            "301 Moved Permanently",
            &[],
            b"",
        ))]);
        let limits = quick();

        let fetched = HttpFetcher::new(&limits)
            .fetch(&server.url("a.png"), &limits)
            .expect("a header-less 3xx is still an answer");

        assert!(matches!(fetched, Fetched::Redirect(to) if to.is_empty()));
    }

    /// The status reaches the caller as a status, whichever way it travelled.
    ///
    /// **This pins the outcome, not the setting, and cannot pin the setting.**
    /// Flipping `http_status_as_error` back to `true` leaves this test green,
    /// because [`transport_error`] maps `ureq::Error::StatusCode` onto
    /// [`FetchError::Status`] as well — the two paths converge by design. That
    /// makes the mutant equivalent *at this boundary*, and it is worth saying
    /// so rather than letting the name imply a coverage this assertion does
    /// not have. The setting still earns its place as the reason the decision
    /// is written once in `fetch` instead of twice; the `StatusCode` arm is
    /// what keeps a future ureq default from changing the answer.
    #[test]
    fn test_a_404_arrives_as_a_status_rather_than_a_transport_failure() {
        let server = Server::start(vec![Serve::Raw(response("404 Not Found", &[], b"nope"))]);
        let limits = quick();

        let error = HttpFetcher::new(&limits)
            .fetch(&server.url("missing.png"), &limits)
            .expect_err("a 404 is not a fetch");

        assert!(
            matches!(error, FetchError::Status(404)),
            "a 404 must name its status, not fold into a transport string: {error:?}"
        );
    }

    /// The ceiling is applied by `.limit(..)` before the read, so it is ours.
    /// Serving 64 bytes against an 8-byte limit proves the number in
    /// `FetchLimits` is the operative one and not ureq's own default.
    #[test]
    fn test_a_body_past_the_ceiling_is_refused_rather_than_read() {
        let server = Server::start(vec![Serve::Raw(response("200 OK", &[], &[0u8; 64]))]);
        let limits = FetchLimits {
            max_response_bytes: 8,
            ..quick()
        };

        let error = HttpFetcher::new(&limits)
            .fetch(&server.url("big.png"), &limits)
            .expect_err("64 bytes is past an 8-byte ceiling");

        assert!(
            matches!(error, FetchError::TooLarge { limit: 8 }),
            "the refusal must carry the ceiling that was hit: {error:?}"
        );
    }

    /// `timeout_global` covers the whole exchange, not just the connect — so a
    /// host that opens a socket and then says nothing still ends at a
    /// deadline. This is the slow-trickle case a connect timeout misses.
    #[test]
    fn test_a_host_that_answers_nothing_ends_at_the_request_deadline() {
        let server = Server::start(vec![Serve::Stall(Duration::from_secs(30))]);
        let limits = quick();

        let started = std::time::Instant::now();
        let error = HttpFetcher::new(&limits)
            .fetch(&server.url("slow.png"), &limits)
            .expect_err("a host that never answers cannot succeed");

        assert!(
            matches!(error, FetchError::Timeout),
            "a silent host is a timeout, not a transport string: {error:?}"
        );
        // Not a speed claim — an upper bound on a bound. The assertion above
        // is what the test is for; this only catches a `request` limit that
        // was ignored in favour of some much larger default, which no amount
        // of host load can fake.
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the 500 ms deadline was not applied at all"
        );
    }

    /// A host that would rather not be scraped can only say so to a client it
    /// can name.
    #[test]
    fn test_the_request_names_stele_and_its_version() {
        let server = Server::start(vec![Serve::Raw(response("200 OK", &[], b"x"))]);
        let limits = quick();

        HttpFetcher::new(&limits)
            .fetch(&server.url("a.png"), &limits)
            .expect("the server answers 200");

        let head = server.requests().pop().expect("the server saw the request");
        assert!(
            head.contains(concat!("stele/", env!("CARGO_PKG_VERSION"))),
            "the request must identify stele and its version: {head}"
        );
    }

    /// Nothing is listening on a closed port, and that is neither a timeout
    /// nor a status — the one case that must land in the catch-all without
    /// losing what happened.
    #[test]
    fn test_a_refused_connection_is_a_transport_failure_with_a_reason() {
        // Bind and drop: the port was ours a moment ago, so nothing else has
        // raced onto it, and now nothing is listening.
        let dead = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback is bindable");
        let url = format!("http://{}/a.png", dead.local_addr().expect("bound"));
        drop(dead);
        let limits = quick();

        let error = HttpFetcher::new(&limits)
            .fetch(&url, &limits)
            .expect_err("nothing is listening");

        match &error {
            FetchError::Transport(reason) => assert!(
                !reason.is_empty(),
                "the catch-all must keep ureq's reason, not swallow it"
            ),
            FetchError::Timeout => {}
            other => panic!("a refused connection is not {other:?}"),
        }
        assert!(
            !error.to_string().contains('{'),
            "even the catch-all renders as prose: {error}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production ceiling is `gfx`'s, not a number typed into this file.
    /// Asserted against `gfx::Limits` directly so a change there moves this
    /// too rather than leaving two ceilings that disagree — which is the whole
    /// reason the field is derived instead of literal.
    #[test]
    fn test_the_download_ceiling_is_the_decoders_own_allocation_ceiling() {
        assert_eq!(
            FetchLimits::default().max_response_bytes,
            gfx::Limits::default().max_alloc
        );
    }

    /// Every bound must actually bound something. A zero anywhere here is a
    /// disabled limit wearing a limit's name, and `document_budget` at zero in
    /// particular would make the flag a no-op that still looks wired up.
    #[test]
    fn test_no_production_bound_is_zero_or_unordered() {
        let limits = FetchLimits::default();
        assert!(limits.connect > Duration::ZERO);
        assert!(
            limits.request >= limits.connect,
            "a request must outlast its own connect"
        );
        assert!(
            limits.document_budget >= limits.request,
            "a document must be allowed at least one whole request"
        );
        assert!(limits.max_response_bytes > 0);
    }

    /// `Display` is what a reader would see if these ever reached the status
    /// row, so none of them may leak a `Debug` dump — the same rule
    /// `LoadError` is held to in `loader.rs`.
    #[test]
    fn test_every_failure_renders_as_prose_rather_than_a_debug_dump() {
        let errors = [
            FetchError::Timeout,
            FetchError::TooLarge {
                limit: 256 * 1024 * 1024,
            },
            FetchError::Status(404),
            FetchError::Transport("dns error".to_string()),
            FetchError::UnsupportedScheme("file".to_string()),
            FetchError::TooManyRedirects { cap: 4 },
            FetchError::UnusableRedirect,
            FetchError::NotAnImage,
            FetchError::CacheWrite(std::io::Error::other("disk full")),
            FetchError::BudgetSpent,
        ];
        for error in errors {
            let message = error.to_string();
            assert!(!message.is_empty(), "{error:?} renders as nothing");
            assert!(!message.contains("Os {"), "{message}");
            assert!(!message.contains('{'), "{message}");
        }
        assert_eq!(
            FetchError::TooLarge {
                limit: 256 * 1024 * 1024
            }
            .to_string(),
            "the response is larger than the 256 MiB stele will download"
        );
        assert_eq!(FetchError::Status(404).to_string(), "the host answered 404");
    }

    /// The stand-in for a build with no network stack refuses rather than
    /// panicking or quietly succeeding, so a policy object that somehow exists
    /// without a real client still ends at alt text.
    #[test]
    fn test_the_no_network_fetcher_refuses_every_url() {
        let limits = FetchLimits::default();
        let error = NoFetcher
            .fetch("https://example.invalid/a.png", &limits)
            .unwrap_err();
        assert!(matches!(error, FetchError::UnsupportedScheme(_)));
    }
}
