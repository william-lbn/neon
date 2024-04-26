/// Test postgres_backend_async with tokio_postgres
use once_cell::sync::Lazy;
use postgres_backend::{AuthType, Handler, PostgresBackend, QueryError};
use pq_proto::{BeMessage, RowDescriptor};
use std::io::Cursor;
use std::{future, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio_postgres::config::SslMode;
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::{Config, NoTls, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

// generate client, server test streams
async fn make_tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client_stream = TcpStream::connect(addr).await.unwrap();
    let (server_stream, _) = listener.accept().await.unwrap();
    (client_stream, server_stream)
}

struct TestHandler {}

#[async_trait::async_trait]
impl<IO: AsyncRead + AsyncWrite + Unpin + Send> Handler<IO> for TestHandler {
    // return single col 'hey' for any query
    async fn process_query(
        &mut self,
        pgb: &mut PostgresBackend<IO>,
        _query_string: &str,
    ) -> Result<(), QueryError> {
        pgb.write_message_noflush(&BeMessage::RowDescription(&[RowDescriptor::text_col(
            b"hey",
        )]))?
        .write_message_noflush(&BeMessage::DataRow(&[Some("hey".as_bytes())]))?
        .write_message_noflush(&BeMessage::CommandComplete(b"SELECT 1"))?;
        Ok(())
    }
}

// test that basic select works
#[tokio::test]
async fn simple_select() {
    let (client_sock, server_sock) = make_tcp_pair().await;

    // create and run pgbackend
    let pgbackend =
        PostgresBackend::new(server_sock, AuthType::Trust, None).expect("pgbackend creation");

    tokio::spawn(async move {
        let mut handler = TestHandler {};
        pgbackend.run(&mut handler, future::pending::<()>).await
    });

    let conf = Config::new();
    let (client, connection) = conf.connect_raw(client_sock, NoTls).await.expect("connect");
    // The connection object performs the actual communication with the database,
    // so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let first_val = &(client.simple_query("SELECT 42;").await.expect("select"))[0];
    if let SimpleQueryMessage::Row(row) = first_val {
        let first_col = row.get(0).expect("first column");
        assert_eq!(first_col, "hey");
    } else {
        panic!("expected SimpleQueryMessage::Row");
    }
}

static KEY: Lazy<rustls::PrivateKey> = Lazy::new(|| {
    let mut cursor = Cursor::new(include_bytes!("key.pem"));
    rustls::PrivateKey(rustls_pemfile::rsa_private_keys(&mut cursor).unwrap()[0].clone())
});

static CERT: Lazy<rustls::Certificate> = Lazy::new(|| {
    let mut cursor = Cursor::new(include_bytes!("cert.pem"));
    rustls::Certificate(rustls_pemfile::certs(&mut cursor).unwrap()[0].clone())
});

// test that basic select with ssl works
#[tokio::test]
async fn simple_select_ssl() {
    let (client_sock, server_sock) = make_tcp_pair().await;

    let server_cfg = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![CERT.clone()], KEY.clone())
        .unwrap();
    let tls_config = Some(Arc::new(server_cfg));
    let pgbackend =
        PostgresBackend::new(server_sock, AuthType::Trust, tls_config).expect("pgbackend creation");

    tokio::spawn(async move {
        let mut handler = TestHandler {};
        pgbackend.run(&mut handler, future::pending::<()>).await
    });

    let client_cfg = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates({
            let mut store = rustls::RootCertStore::empty();
            store.add(&CERT).unwrap();
            store
        })
        .with_no_client_auth();
    let mut make_tls_connect = tokio_postgres_rustls::MakeRustlsConnect::new(client_cfg);
    let tls_connect = <MakeRustlsConnect as MakeTlsConnect<TcpStream>>::make_tls_connect(
        &mut make_tls_connect,
        "localhost",
    )
    .expect("make_tls_connect");

    let mut conf = Config::new();
    conf.ssl_mode(SslMode::Require);
    let (client, connection) = conf
        .connect_raw(client_sock, tls_connect)
        .await
        .expect("connect");
    // The connection object performs the actual communication with the database,
    // so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let first_val = &(client.simple_query("SELECT 42;").await.expect("select"))[0];
    if let SimpleQueryMessage::Row(row) = first_val {
        let first_col = row.get(0).expect("first column");
        assert_eq!(first_col, "hey");
    } else {
        panic!("expected SimpleQueryMessage::Row");
    }
}
