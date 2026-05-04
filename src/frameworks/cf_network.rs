/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Stub for `CFNetwork.framework/CFNetwork`.
//!
//! Purpose:
//! - satisfy symbol lookup so apps that depend on CFNetwork don't crash
//! - avoid crashes on simple create/open checks
//! - behave safely when the game polls streams or hosts

//!
//! Note: CFHost* functions are implemented in
//! [crate::frameworks::core_foundation::cf_host] and are registered with
//! CoreFoundation's HostDylib. On a real device they live in CFNetwork, but
//! touchHLE's dyld searches all frameworks so the binding resolves regardless
//! of which framework the app links against. Keeping them only in one place
//! avoids `no_duplicate_functions` test failures.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::mem::MutPtr;
use crate::Environment;

const DUMMY_STREAM: u32 = 0xC0F0_0001;

fn CFReadStreamCreateForHTTPRequest(_env: &mut Environment, _alloc: u32, _request: u32) -> u32 {
    // Return a non-null dummy handle so callers that only check for null
    // continue past the nil check.
    DUMMY_STREAM
}

fn CFReadStreamOpen(_env: &mut Environment, _stream: u32) -> bool {
    true
}

fn CFReadStreamHasBytesAvailable(_env: &mut Environment, _stream: u32) -> bool {
    false
}

fn CFReadStreamRead(
    _env: &mut Environment,
    _stream: u32,
    _buffer: MutPtr<u8>,
    _buffer_length: i32,
) -> i32 {
    0
}

fn CFReadStreamClose(_env: &mut Environment, _stream: u32) {}

fn CFReadStreamSetProperty(
    _env: &mut Environment,
    _stream: u32,
    _property: u32,
    _value: u32,
) -> bool {
    true
}

fn CFReadStreamCopyProperty(_env: &mut Environment, _stream: u32, _property: u32) -> u32 {
    0
}

fn CFReadStreamScheduleWithRunLoop(
    _env: &mut Environment,
    _stream: u32,
    _run_loop: u32,
    _run_loop_mode: u32,
) {
}

fn CFReadStreamUnscheduleFromRunLoop(
    _env: &mut Environment,
    _stream: u32,
    _run_loop: u32,
    _run_loop_mode: u32,
) {
}

fn CFReadStreamSetClient(
    _env: &mut Environment,
    _stream: u32,
    _callback_types: u32,
    _client_cb: u32,
    _client_context: u32,
) -> bool {
    true
}

fn CFReadStreamGetStatus(_env: &mut Environment, _stream: u32) -> u32 {
    // kCFStreamStatusOpen
    2
}

fn CFReadStreamCopyError(_env: &mut Environment, _stream: u32) -> u32 {
    0
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFReadStreamCreateForHTTPRequest(_, _)),
    // Other CFReadStream* helpers are exported from
    // core_foundation::cf_stream; not duplicated here.
    export_c_func!(CFReadStreamCopyError(_)),
];

// CFNetwork & related non-lazy NSString constants.
//
// Every entry below is a constant `CFStringRef` exported by CFNetwork (or, in
// the case of the kCFErrorDomain* / kUTTagClass* ones, by CoreServices /
// CoreFoundation but pulled in transitively by CFNetwork). Apps include them
// in `__nl_symbol_ptr` even when the actual networking code is never reached
// (e.g. Authenticate-Anything boilerplate baked into Game Center / Mono / ARC
// runtimes), so missing them prints a `Warning: unhandled non-lazy symbol …`
// and zero-fills the slot — which then later trips up `CFEqual` / NSDictionary
// keying with `0x0`. Exporting them as plain unique NSStrings is enough to
// silence the warnings and keep the dictionary keys distinct.
//
// Values are the documented CFString contents from the Apple SDK headers.
// Where the runtime doesn't observe the literal value (purely sentinel CFTypes)
// we still pick the conventional spelling for readability.
pub const CONSTANTS: ConstantExports = &[
    // NSURLAuthenticationChallenge / NSURLProtectionSpace authentication-method names.
    (
        "_NSURLAuthenticationMethodDefault",
        HostConstant::NSString("NSURLAuthenticationMethodDefault"),
    ),
    (
        "_NSURLAuthenticationMethodHTTPBasic",
        HostConstant::NSString("NSURLAuthenticationMethodHTTPBasic"),
    ),
    (
        "_NSURLAuthenticationMethodHTTPDigest",
        HostConstant::NSString("NSURLAuthenticationMethodHTTPDigest"),
    ),
    (
        "_NSURLAuthenticationMethodHTMLForm",
        HostConstant::NSString("NSURLAuthenticationMethodHTMLForm"),
    ),
    (
        "_NSURLAuthenticationMethodNTLM",
        HostConstant::NSString("NSURLAuthenticationMethodNTLM"),
    ),
    (
        "_NSURLAuthenticationMethodNegotiate",
        HostConstant::NSString("NSURLAuthenticationMethodNegotiate"),
    ),
    (
        "_NSURLAuthenticationMethodClientCertificate",
        HostConstant::NSString("NSURLAuthenticationMethodClientCertificate"),
    ),
    (
        "_NSURLAuthenticationMethodServerTrust",
        HostConstant::NSString("NSURLAuthenticationMethodServerTrust"),
    ),
    // NSURLProtectionSpace proxy-type identifiers.
    (
        "_NSURLProtectionSpaceHTTP",
        HostConstant::NSString("NSURLProtectionSpaceHTTP"),
    ),
    (
        "_NSURLProtectionSpaceHTTPS",
        HostConstant::NSString("NSURLProtectionSpaceHTTPS"),
    ),
    (
        "_NSURLProtectionSpaceFTP",
        HostConstant::NSString("NSURLProtectionSpaceFTP"),
    ),
    (
        "_NSURLProtectionSpaceHTTPProxy",
        HostConstant::NSString("NSURLProtectionSpaceHTTPProxy"),
    ),
    (
        "_NSURLProtectionSpaceHTTPSProxy",
        HostConstant::NSString("NSURLProtectionSpaceHTTPSProxy"),
    ),
    (
        "_NSURLProtectionSpaceFTPProxy",
        HostConstant::NSString("NSURLProtectionSpaceFTPProxy"),
    ),
    (
        "_NSURLProtectionSpaceSOCKSProxy",
        HostConstant::NSString("NSURLProtectionSpaceSOCKSProxy"),
    ),
    // CFNetwork errors.
    (
        "_kCFErrorDomainCFNetwork",
        HostConstant::NSString("kCFErrorDomainCFNetwork"),
    ),
    // CFHTTPMessage / CFHTTPAuthentication property keys (kCFHTTP*).
    ("_kCFHTTPVersion1_0", HostConstant::NSString("HTTP/1.0")),
    ("_kCFHTTPVersion1_1", HostConstant::NSString("HTTP/1.1")),
    (
        "_kCFHTTPAuthenticationSchemeBasic",
        HostConstant::NSString("Basic"),
    ),
    (
        "_kCFHTTPAuthenticationSchemeDigest",
        HostConstant::NSString("Digest"),
    ),
    (
        "_kCFHTTPAuthenticationSchemeNTLM",
        HostConstant::NSString("NTLM"),
    ),
    (
        "_kCFHTTPAuthenticationSchemeNegotiate",
        HostConstant::NSString("Negotiate"),
    ),
    (
        "_kCFHTTPAuthenticationUsername",
        HostConstant::NSString("kCFHTTPAuthenticationUsername"),
    ),
    (
        "_kCFHTTPAuthenticationPassword",
        HostConstant::NSString("kCFHTTPAuthenticationPassword"),
    ),
    (
        "_kCFHTTPAuthenticationAccountDomain",
        HostConstant::NSString("kCFHTTPAuthenticationAccountDomain"),
    ),
    // CFProxy lookup-result dictionary keys.
    (
        "_kCFProxyAutoConfigurationURLKey",
        HostConstant::NSString("kCFProxyAutoConfigurationURLKey"),
    ),
    (
        "_kCFProxyHostNameKey",
        HostConstant::NSString("kCFProxyHostNameKey"),
    ),
    (
        "_kCFProxyPortNumberKey",
        HostConstant::NSString("kCFProxyPortNumberKey"),
    ),
    (
        "_kCFProxyTypeKey",
        HostConstant::NSString("kCFProxyTypeKey"),
    ),
    (
        "_kCFProxyTypeNone",
        HostConstant::NSString("kCFProxyTypeNone"),
    ),
    (
        "_kCFProxyTypeHTTP",
        HostConstant::NSString("kCFProxyTypeHTTP"),
    ),
    (
        "_kCFProxyTypeHTTPS",
        HostConstant::NSString("kCFProxyTypeHTTPS"),
    ),
    (
        "_kCFProxyTypeSOCKS",
        HostConstant::NSString("kCFProxyTypeSOCKS"),
    ),
    (
        "_kCFProxyTypeFTP",
        HostConstant::NSString("kCFProxyTypeFTP"),
    ),
    (
        "_kCFProxyTypeAutoConfigurationURL",
        HostConstant::NSString("kCFProxyTypeAutoConfigurationURL"),
    ),
    // CFStream property names.
    (
        "_kCFStreamPropertyHTTPProxy",
        HostConstant::NSString("kCFStreamPropertyHTTPProxy"),
    ),
    (
        "_kCFStreamPropertyHTTPProxyHost",
        HostConstant::NSString("kCFStreamPropertyHTTPProxyHost"),
    ),
    (
        "_kCFStreamPropertyHTTPProxyPort",
        HostConstant::NSString("kCFStreamPropertyHTTPProxyPort"),
    ),
    (
        "_kCFStreamPropertyHTTPSProxyHost",
        HostConstant::NSString("kCFStreamPropertyHTTPSProxyHost"),
    ),
    (
        "_kCFStreamPropertyHTTPSProxyPort",
        HostConstant::NSString("kCFStreamPropertyHTTPSProxyPort"),
    ),
    (
        "_kCFStreamPropertySOCKSProxy",
        HostConstant::NSString("kCFStreamPropertySOCKSProxy"),
    ),
    (
        "_kCFStreamPropertySOCKSProxyHost",
        HostConstant::NSString("kCFStreamPropertySOCKSProxyHost"),
    ),
    (
        "_kCFStreamPropertySOCKSProxyPort",
        HostConstant::NSString("kCFStreamPropertySOCKSProxyPort"),
    ),
    (
        "_kCFStreamPropertyHTTPAttemptPersistentConnection",
        HostConstant::NSString("kCFStreamPropertyHTTPAttemptPersistentConnection"),
    ),
    (
        "_kCFStreamPropertyHTTPRequestBytesWrittenCount",
        HostConstant::NSString("kCFStreamPropertyHTTPRequestBytesWrittenCount"),
    ),
    (
        "_kCFStreamPropertyHTTPResponseHeader",
        HostConstant::NSString("kCFStreamPropertyHTTPResponseHeader"),
    ),
    (
        "_kCFStreamPropertyHTTPShouldAutoredirect",
        HostConstant::NSString("kCFStreamPropertyHTTPShouldAutoredirect"),
    ),
    (
        "_kCFStreamPropertyHTTPFinalURL",
        HostConstant::NSString("kCFStreamPropertyHTTPFinalURL"),
    ),
    // CFStream socket-security-level identifiers.
    (
        "_kCFStreamSocketSecurityLevelNone",
        HostConstant::NSString("kCFStreamSocketSecurityLevelNone"),
    ),
    (
        "_kCFStreamSocketSecurityLevelSSLv2",
        HostConstant::NSString("kCFStreamSocketSecurityLevelSSLv2"),
    ),
    (
        "_kCFStreamSocketSecurityLevelSSLv3",
        HostConstant::NSString("kCFStreamSocketSecurityLevelSSLv3"),
    ),
    (
        "_kCFStreamSocketSecurityLevelTLSv1",
        HostConstant::NSString("kCFStreamSocketSecurityLevelTLSv1"),
    ),
    (
        "_kCFStreamSocketSecurityLevelNegotiatedSSL",
        HostConstant::NSString("kCFStreamSocketSecurityLevelNegotiatedSSL"),
    ),
    // CFStream error-domain identifiers (also exposed by CoreServices).
    (
        "_kCFStreamErrorDomainHTTP",
        HostConstant::NSString("kCFStreamErrorDomainHTTP"),
    ),
    (
        "_kCFStreamErrorDomainSSL",
        HostConstant::NSString("kCFStreamErrorDomainSSL"),
    ),
    (
        "_kCFStreamErrorDomainSOCKS",
        HostConstant::NSString("kCFStreamErrorDomainSOCKS"),
    ),
    (
        "_kCFStreamErrorDomainSystemConfiguration",
        HostConstant::NSString("kCFStreamErrorDomainSystemConfiguration"),
    ),
    (
        "_kCFStreamErrorDomainNetServices",
        HostConstant::NSString("kCFStreamErrorDomainNetServices"),
    ),
    (
        "_kCFStreamErrorDomainMach",
        HostConstant::NSString("kCFStreamErrorDomainMach"),
    ),
];
