// Copyright (c) The Libra Core Contributors
// SPDX-License-Identifier: Apache-2.0

//
// Noise Fuzzing
// =============
//
// This fuzzes the wrappers we have around our Noise library.
//

use crate::noise::{AntiReplayTimestamps, HandshakeAuthMode, NoiseUpgrader};
use futures::{
    executor::block_on,
    future::join,
    io::{AsyncRead, AsyncWrite},
    ready,
    task::{Context, Poll},
};
use libra_crypto::{test_utils::TEST_SEED, x25519, Uniform as _};
use libra_types::PeerId;
use memsocket::MemorySocket;
use once_cell::sync::Lazy;
use rand_core::SeedableRng;
use std::{io, pin::Pin};

//
// Corpus generation
// =================
//
// - ExposingSocket: a wrapper around MemorySocket to retrieve handshake messages being sent.
// - KEYPAIR: a unique keypair for fuzzing
// - generate_first_two_messages: it will generate the first or second message in the handshake.
// - generate_corpus: the function called by our fuzzer to retrieve the corpus.
//

// ExposingSocket is needed to retrieve the noise messages peers will write on the socket
struct ExposingSocket {
    pub inner: MemorySocket,
    pub written: Vec<u8>,
}
impl ExposingSocket {
    fn new(memory_socket: MemorySocket) -> Self {
        Self {
            inner: memory_socket,
            written: Vec::new(),
        }
    }
    fn new_pair() -> (Self, Self) {
        let (dialer_socket, listener_socket) = MemorySocket::new_pair();
        (
            ExposingSocket::new(dialer_socket),
            ExposingSocket::new(listener_socket),
        )
    }
}
impl AsyncWrite for ExposingSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let bytes_written = ready!(Pin::new(&mut self.inner).poll_write(context, buf))?;
        self.written.extend_from_slice(&buf[..bytes_written]);
        Poll::Ready(Ok(bytes_written))
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    /// Attempt to close the channel. Cannot Fail.
    fn poll_close(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(context)
    }
}
impl AsyncRead for ExposingSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(context, buf)
    }
}

//
// Actual code to generate corpus
//

// let's cache the deterministic keypair
pub static KEYPAIR: Lazy<(x25519::PrivateKey, x25519::PublicKey)> = Lazy::new(|| {
    let mut rng = ::rand::rngs::StdRng::from_seed(TEST_SEED);
    let private_key = x25519::PrivateKey::generate(&mut rng);
    let public_key = private_key.public_key();
    (private_key, public_key)
});

fn generate_first_two_messages() -> (Vec<u8>, Vec<u8>) {
    // build
    let (private_key, public_key) = KEYPAIR.clone();
    let peer_id = PeerId::from_identity_public_key(public_key);
    let initiator = NoiseUpgrader::new(peer_id, private_key.clone(), HandshakeAuthMode::ServerOnly);
    let responder = NoiseUpgrader::new(peer_id, private_key, HandshakeAuthMode::ServerOnly);

    // create exposing socket
    let (dialer_socket, listener_socket) = ExposingSocket::new_pair();

    // perform the handshake
    let (client_session, server_session) = block_on(join(
        initiator.upgrade_outbound(dialer_socket, public_key, fake_timestamp),
        responder.upgrade_inbound(listener_socket),
    ));

    // take result
    let client_session = client_session.unwrap();
    let (server_session, _peer_id) = server_session.unwrap();
    let init_msg = client_session.into_socket().written;
    let resp_msg = server_session.into_socket().written;

    (init_msg, resp_msg)
}

pub fn generate_corpus(gen: &mut libra_proptest_helpers::ValueGenerator) -> Vec<u8> {
    let (init_msg, resp_msg) = generate_first_two_messages();
    // choose a random one
    let strategy = proptest::arbitrary::any::<bool>();
    if gen.generate(strategy) {
        init_msg
    } else {
        resp_msg
    }
}

//
// Fuzzing
// =======
//
// - FakeSocket: used to quickly consume fuzzing data and dismiss socket writes.
// - fuzz_initiator: fuzzes the second message of the handshake, received by the initiator.
// - fuzz_responder: fuzzes the first message of the handshake, received by the responder.
//

struct FakeSocket<'a> {
    pub content: &'a [u8],
}
impl<'a> AsyncWrite for FakeSocket<'a> {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// Attempt to close the channel. Cannot Fail.
    fn poll_close(self: Pin<&mut Self>, _context: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl<'a> AsyncRead for FakeSocket<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        mut _context: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        // if nothing left, just return 0
        if self.content.is_empty() {
            return Poll::Ready(Ok(0));
        }
        // if something left, return what's asked
        let to_read = std::cmp::min(buf.len(), self.content.len());
        buf[..to_read].copy_from_slice(&self.content[..to_read]);
        // update internal state
        self.content = &self.content[to_read..self.content.len()];
        // return length read
        Poll::Ready(Ok(to_read))
    }
}

/// let's provide the same timestamp everytime, faster
fn fake_timestamp() -> [u8; AntiReplayTimestamps::TIMESTAMP_SIZE] {
    [0u8; AntiReplayTimestamps::TIMESTAMP_SIZE]
}

pub fn fuzz_initiator(data: &[u8]) {
    // setup initiator
    let (private_key, public_key) = KEYPAIR.clone();
    let peer_id = PeerId::from_identity_public_key(public_key);
    let initiator = NoiseUpgrader::new(peer_id, private_key, HandshakeAuthMode::ServerOnly);

    // setup NoiseStream
    let fake_socket = FakeSocket { content: data };

    // send a message, then read fuzz data
    let _ = block_on(initiator.upgrade_outbound(fake_socket, public_key, fake_timestamp));
}

pub fn fuzz_responder(data: &[u8]) {
    // setup responder
    let (private_key, public_key) = KEYPAIR.clone();
    let peer_id = PeerId::from_identity_public_key(public_key);
    let responder = NoiseUpgrader::new(peer_id, private_key, HandshakeAuthMode::ServerOnly);

    // setup NoiseStream
    let fake_socket = FakeSocket { content: data };

    // read fuzz data
    let _ = block_on(responder.upgrade_inbound(fake_socket));
}

//
// Tests
// =====
//
// To ensure fuzzers will not break, this test the fuzzers.
//

#[test]
fn test_noise_fuzzer() {
    let (init_msg, resp_msg) = generate_first_two_messages();
    fuzz_responder(&init_msg);
    fuzz_initiator(&resp_msg);
}