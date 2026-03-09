// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! WebCrypto API wrapper for WASM builds.
//!
//! Uses browser's native SubtleCrypto for Ed25519, SHA-256, HMAC, HKDF,
//! PBKDF2, X25519. ChaCha20-Poly1305 and Argon2 use RustCrypto (no browser support).
//!
//! All WebCrypto operations are async (return Promise). This module provides
//! blocking wrappers via wasm-bindgen-futures for compatibility with the
//! sync WorkflowEngine API.

#![cfg(feature = "crypto-wasm")]

use js_sys::{ArrayBuffer, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Crypto, CryptoKey, SubtleCrypto};

fn get_crypto() -> Crypto {
    // Try window (main thread) first, fall back to WorkerGlobalScope (Web Workers)
    if let Some(window) = web_sys::window() {
        return window.crypto().expect("no crypto on window");
    }
    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    global.crypto().expect("no crypto on worker")
}

fn get_subtle() -> SubtleCrypto {
    get_crypto().subtle()
}

/// Fill buffer with cryptographically secure random bytes.
pub fn random_fill(buf: &mut [u8]) {
    let crypto = get_crypto();
    let array = Uint8Array::new_with_length(buf.len() as u32);
    crypto
        .get_random_values_with_u8_array(&array)
        .expect("getRandomValues failed");
    array.copy_to(buf);
}

/// SHA-256 digest (async internally, but we need sync for core API).
pub async fn sha256(data: &[u8]) -> [u8; 32] {
    let subtle = get_subtle();
    let data_array = Uint8Array::from(data);

    let promise = subtle
        .digest_with_str_and_buffer_source("SHA-256", &data_array)
        .expect("digest failed");
    let result = JsFuture::from(promise).await.expect("digest await failed");

    let buffer = ArrayBuffer::from(result);
    let array = Uint8Array::new(&buffer);
    let mut output = [0u8; 32];
    array.copy_to(&mut output);
    output
}

/// HMAC-SHA256 sign.
pub async fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let subtle = get_subtle();

    // Import key
    let key_data = Uint8Array::from(key);
    let algorithm = js_sys::JSON::parse(r#"{"name":"HMAC","hash":"SHA-256"}"#).unwrap();
    let key_promise = subtle
        .import_key_with_object(
            "raw",
            &key_data,
            &algorithm.into(),
            false,
            &js_sys::Array::of1(&JsValue::from_str("sign")),
        )
        .expect("import_key failed");
    let crypto_key: CryptoKey = JsFuture::from(key_promise)
        .await
        .expect("key import failed")
        .into();

    // Sign
    let data_array = Uint8Array::from(data);
    let sign_promise = subtle
        .sign_with_str_and_buffer_source("HMAC", &crypto_key, &data_array)
        .expect("sign failed");
    let result = JsFuture::from(sign_promise)
        .await
        .expect("sign await failed");

    let buffer = ArrayBuffer::from(result);
    let array = Uint8Array::new(&buffer);
    let mut output = [0u8; 32];
    array.copy_to(&mut output);
    output
}

/// HKDF-SHA256 derive.
pub async fn hkdf_derive(salt: &[u8], ikm: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let subtle = get_subtle();

    // Import IKM as key material
    let ikm_data = Uint8Array::from(ikm);
    let import_algo = js_sys::JSON::parse(r#"{"name":"HKDF"}"#).unwrap();
    let key_promise = subtle
        .import_key_with_object(
            "raw",
            &ikm_data,
            &import_algo.into(),
            false,
            &js_sys::Array::of1(&JsValue::from_str("deriveBits")),
        )
        .expect("import_key failed");
    let crypto_key: CryptoKey = JsFuture::from(key_promise)
        .await
        .expect("key import failed")
        .into();

    // Derive bits
    let derive_params = Object::new();
    Reflect::set(&derive_params, &"name".into(), &"HKDF".into()).unwrap();
    Reflect::set(&derive_params, &"hash".into(), &"SHA-256".into()).unwrap();
    Reflect::set(
        &derive_params,
        &"salt".into(),
        &Uint8Array::from(salt).into(),
    )
    .unwrap();
    Reflect::set(
        &derive_params,
        &"info".into(),
        &Uint8Array::from(info).into(),
    )
    .unwrap();

    let derive_promise = subtle
        .derive_bits_with_object(&derive_params, &crypto_key, (length * 8) as u32)
        .expect("deriveBits failed");
    let result = JsFuture::from(derive_promise)
        .await
        .expect("derive await failed");

    let buffer = ArrayBuffer::from(result);
    let array = Uint8Array::new(&buffer);
    array.to_vec()
}

/// PBKDF2-HMAC-SHA256 derive.
pub async fn pbkdf2_derive(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    length: usize,
) -> Vec<u8> {
    let subtle = get_subtle();

    let pw_data = Uint8Array::from(password);
    let import_algo = js_sys::JSON::parse(r#"{"name":"PBKDF2"}"#).unwrap();
    let key_promise = subtle
        .import_key_with_object(
            "raw",
            &pw_data,
            &import_algo.into(),
            false,
            &js_sys::Array::of1(&JsValue::from_str("deriveBits")),
        )
        .expect("import_key failed");
    let crypto_key: CryptoKey = JsFuture::from(key_promise)
        .await
        .expect("key import failed")
        .into();

    let derive_params = Object::new();
    Reflect::set(&derive_params, &"name".into(), &"PBKDF2".into()).unwrap();
    Reflect::set(&derive_params, &"hash".into(), &"SHA-256".into()).unwrap();
    Reflect::set(
        &derive_params,
        &"salt".into(),
        &Uint8Array::from(salt).into(),
    )
    .unwrap();
    Reflect::set(
        &derive_params,
        &"iterations".into(),
        &JsValue::from(iterations),
    )
    .unwrap();

    let derive_promise = subtle
        .derive_bits_with_object(&derive_params, &crypto_key, (length * 8) as u32)
        .expect("deriveBits failed");
    let result = JsFuture::from(derive_promise)
        .await
        .expect("derive await failed");

    let buffer = ArrayBuffer::from(result);
    Uint8Array::new(&buffer).to_vec()
}

/// Ed25519 key generation (returns (public_key, private_key) as raw bytes).
pub async fn ed25519_generate() -> ([u8; 32], Vec<u8>) {
    let subtle = get_subtle();

    let algo = js_sys::JSON::parse(r#"{"name":"Ed25519"}"#).unwrap();
    let gen_promise = subtle
        .generate_key_with_object(
            &algo.into(),
            true, // extractable
            &js_sys::Array::of2(&"sign".into(), &"verify".into()),
        )
        .expect("generateKey failed");
    let key_pair = JsFuture::from(gen_promise).await.expect("keygen failed");

    // Extract public key
    let public_key: CryptoKey = Reflect::get(&key_pair, &"publicKey".into()).unwrap().into();
    let pub_export = JsFuture::from(
        subtle
            .export_key("raw", &public_key)
            .expect("export failed"),
    )
    .await
    .expect("export failed");
    let pub_bytes = Uint8Array::new(&ArrayBuffer::from(pub_export));
    let mut pub_out = [0u8; 32];
    pub_bytes.copy_to(&mut pub_out);

    // Extract private key (PKCS8 format)
    let private_key: CryptoKey = Reflect::get(&key_pair, &"privateKey".into())
        .unwrap()
        .into();
    let priv_export = JsFuture::from(
        subtle
            .export_key("pkcs8", &private_key)
            .expect("export failed"),
    )
    .await
    .expect("export failed");
    let priv_bytes = Uint8Array::new(&ArrayBuffer::from(priv_export));

    (pub_out, priv_bytes.to_vec())
}

/// Ed25519 sign.
pub async fn ed25519_sign(private_key_pkcs8: &[u8], message: &[u8]) -> Vec<u8> {
    let subtle = get_subtle();

    let key_data = Uint8Array::from(private_key_pkcs8);
    let algo = js_sys::JSON::parse(r#"{"name":"Ed25519"}"#).unwrap();
    let key_promise = subtle
        .import_key_with_object(
            "pkcs8",
            &key_data,
            &algo.into(),
            false,
            &js_sys::Array::of1(&"sign".into()),
        )
        .expect("import_key failed");
    let crypto_key: CryptoKey = JsFuture::from(key_promise)
        .await
        .expect("import failed")
        .into();

    let msg_data = Uint8Array::from(message);
    let sign_promise = subtle
        .sign_with_str_and_buffer_source("Ed25519", &crypto_key, &msg_data)
        .expect("sign failed");
    let result = JsFuture::from(sign_promise).await.expect("sign failed");

    Uint8Array::new(&ArrayBuffer::from(result)).to_vec()
}

/// Ed25519 verify.
pub async fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let subtle = get_subtle();

    let key_data = Uint8Array::from(public_key);
    let algo = js_sys::JSON::parse(r#"{"name":"Ed25519"}"#).unwrap();
    let key_promise = subtle
        .import_key_with_object(
            "raw",
            &key_data,
            &algo.into(),
            false,
            &js_sys::Array::of1(&"verify".into()),
        )
        .expect("import_key failed");
    let crypto_key: CryptoKey = JsFuture::from(key_promise)
        .await
        .expect("import failed")
        .into();

    let msg_data = Uint8Array::from(message);
    let sig_data = Uint8Array::from(signature);
    let verify_promise = subtle
        .verify_with_str_and_buffer_source_and_buffer_source(
            "Ed25519",
            &crypto_key,
            &sig_data,
            &msg_data,
        )
        .expect("verify failed");
    let result = JsFuture::from(verify_promise).await.expect("verify failed");

    result.as_bool().unwrap_or(false)
}

/// X25519 key generation (returns (public_key, private_key) as raw bytes).
pub async fn x25519_generate() -> ([u8; 32], Vec<u8>) {
    let subtle = get_subtle();

    let algo = js_sys::JSON::parse(r#"{"name":"X25519"}"#).unwrap();
    let gen_promise = subtle
        .generate_key_with_object(
            &algo.into(),
            true,
            &js_sys::Array::of1(&"deriveBits".into()),
        )
        .expect("generateKey failed");
    let key_pair = JsFuture::from(gen_promise).await.expect("keygen failed");

    let public_key: CryptoKey = Reflect::get(&key_pair, &"publicKey".into()).unwrap().into();
    let pub_export = JsFuture::from(
        subtle
            .export_key("raw", &public_key)
            .expect("export failed"),
    )
    .await
    .expect("export failed");
    let pub_bytes = Uint8Array::new(&ArrayBuffer::from(pub_export));
    let mut pub_out = [0u8; 32];
    pub_bytes.copy_to(&mut pub_out);

    let private_key: CryptoKey = Reflect::get(&key_pair, &"privateKey".into())
        .unwrap()
        .into();
    let priv_export = JsFuture::from(
        subtle
            .export_key("pkcs8", &private_key)
            .expect("export failed"),
    )
    .await
    .expect("export failed");
    let priv_bytes = Uint8Array::new(&ArrayBuffer::from(priv_export));

    (pub_out, priv_bytes.to_vec())
}

/// X25519 key agreement (Diffie-Hellman).
pub async fn x25519_derive(private_key_pkcs8: &[u8], public_key: &[u8]) -> [u8; 32] {
    let subtle = get_subtle();

    // Import private key
    let priv_data = Uint8Array::from(private_key_pkcs8);
    let algo = js_sys::JSON::parse(r#"{"name":"X25519"}"#).unwrap();
    let priv_promise = subtle
        .import_key_with_object(
            "pkcs8",
            &priv_data,
            &algo.into(),
            false,
            &js_sys::Array::of1(&"deriveBits".into()),
        )
        .expect("import failed");
    let priv_key: CryptoKey = JsFuture::from(priv_promise)
        .await
        .expect("import failed")
        .into();

    // Import public key
    let pub_data = Uint8Array::from(public_key);
    let pub_algo = js_sys::JSON::parse(r#"{"name":"X25519"}"#).unwrap();
    let pub_promise = subtle
        .import_key_with_object(
            "raw",
            &pub_data,
            &pub_algo.into(),
            false,
            &js_sys::Array::new(),
        )
        .expect("import failed");
    let pub_key: CryptoKey = JsFuture::from(pub_promise)
        .await
        .expect("import failed")
        .into();

    // Derive bits
    let derive_params = Object::new();
    Reflect::set(&derive_params, &"name".into(), &"X25519".into()).unwrap();
    Reflect::set(&derive_params, &"public".into(), &pub_key.into()).unwrap();

    let derive_promise = subtle
        .derive_bits_with_object(&derive_params, &priv_key, 256)
        .expect("deriveBits failed");
    let result = JsFuture::from(derive_promise).await.expect("derive failed");

    let buffer = ArrayBuffer::from(result);
    let array = Uint8Array::new(&buffer);
    let mut output = [0u8; 32];
    array.copy_to(&mut output);
    output
}
