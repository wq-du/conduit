mod support;
use self::support::*;

#[test]
fn h2_goaways_connections() {
    let _ = env_logger::try_init();

    let (shdn, rx) = shutdown_signal();

    let srv = server::http2().route("/", "hello").run();
    let ctrl = controller::new().run();
    let proxy = proxy::new()
        .controller(ctrl)
        .inbound(srv)
        .shutdown_signal(rx)
        .run();
    let mut client = client::http2(proxy.inbound, "shutdown.test.svc.cluster.local");

    assert_eq!(client.get("/"), "hello");

    shdn.signal();

    client.running().wait().unwrap();
}

#[test]
fn http1_closes_idle_connections() {
    let _ = env_logger::try_init();

    let (shdn, rx) = shutdown_signal();

    let srv = server::http1().route("/", "hello").run();
    let ctrl = controller::new().run();
    let proxy = proxy::new()
        .controller(ctrl)
        .inbound(srv)
        .shutdown_signal(rx)
        .run();
    let mut client = client::http1(proxy.inbound, "shutdown.test.svc.cluster.local");

    assert_eq!(client.get("/"), "hello");

    shdn.signal();

    client.running().wait().unwrap();
}

#[test]
fn tcp_waits_for_proxies_to_close() {
    let _ = env_logger::try_init();

    let (shdn, rx) = shutdown_signal();
    let msg1 = "custom tcp hello";
    let msg2 = "custom tcp bye";

    let srv = server::tcp()
        .accept(move |read| {
            assert_eq!(read, msg1.as_bytes());
            msg2
        })
        .run();
    let ctrl = controller::new().run();
    let proxy = proxy::new()
        .controller(ctrl)
        .inbound(srv)
        .shutdown_signal(rx)
        .run();

    let client = client::tcp(proxy.inbound);

    let tcp_client = client.connect();

    shdn.signal();

    tcp_client.write(msg1);
    assert_eq!(tcp_client.read(), msg2.as_bytes());
}
