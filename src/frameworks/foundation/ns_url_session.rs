/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! `NSURLSession`, `NSURLSessionConfiguration` and `NSURLSessionTask`
//! (iOS 7+ networking). HTTP/HTTPS requests use the host transport when
//! `--allow-network-access` is enabled. Workers operate on copied Rust request
//! data and results are delivered on the guest run loop, never from a host
//! worker thread.
//!
//! The shape of the API faithfully follows Apple's reference:
//! - `+[NSURLSessionConfiguration defaultSessionConfiguration]` /
//!   `ephemeralSessionConfiguration` /
//!   `backgroundSessionConfigurationWithIdentifier:`
//! - `+[NSURLSession sessionWithConfiguration:]`,
//!   `+[NSURLSession sessionWithConfiguration:delegate:delegateQueue:]`,
//!   `+[NSURLSession sharedSession]`
//! - Task creation: `dataTaskWithRequest:`, `dataTaskWithURL:` and their
//!   `completionHandler:` variants (plus download/upload spellings).
//! - `NSURLSessionTask`: `resume`, `suspend`, `cancel`, and the read-only
//!   `state`, `taskIdentifier`, `originalRequest`, `currentRequest`, `error`.
//!
//! HTTP status codes (including 4xx/5xx) are returned as normal responses;
//! only transport failures are reported through NSError. Background sessions
//! are executed in-process and do not survive emulator shutdown.
use super::NSUInteger;
use crate::abi::{CallFromHost, GuestFunction};
use crate::frameworks::foundation::url_loading;
use crate::mem::{ConstVoidPtr, Ptr};
use crate::objc::{
    autorelease, id, msg, msg_class, msg_super, nil, objc_classes, release, retain, ClassExports,
    HostObject,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::TryRecvError;

// NSURLSessionTaskState values (Apple's enum).
const NS_URL_SESSION_TASK_STATE_RUNNING: i64 = 0;
const NS_URL_SESSION_TASK_STATE_SUSPENDED: i64 = 1;
const NS_URL_SESSION_TASK_STATE_CANCELING: i64 = 2;
const NS_URL_SESSION_TASK_STATE_COMPLETED: i64 = 3;

/// Apple Block ABI: word offset 3 (== byte offset 12) of a block struct holds
/// its `invoke` function pointer.
/// <https://clang.llvm.org/docs/Block-ABI-Apple.html>
const BLOCK_INVOKE_WORD_OFFSET: u32 = 3;

#[derive(Default)]
pub struct State {
    /// Monotonic counter used to hand out unique task identifiers within this
    /// emulated process.
    next_task_identifier: u64,
    /// Strongly held process-wide `+[NSURLSession sharedSession]` instance.
    shared_session: id,
}
impl State {
    fn get(env: &mut crate::Environment) -> &mut State {
        &mut env.framework_state.foundation.ns_url_session
    }
}

// ---------------------------------------------------------------------------
// Host objects.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct NSURLSessionConfigurationHostObject {
    /// `NSString *` identifier for background configurations (else nil).
    identifier: id,
    allows_cellular_access: bool,
    http_additional_headers: id,
    /// `NSURLCache *`, retained when explicitly supplied by the guest.
    url_cache: id,
    /// NSURLRequestCachePolicy / NSUInteger.
    request_cache_policy: NSUInteger,
    timeout_interval_for_request: f64,
    timeout_interval_for_resource: f64,
}
impl HostObject for NSURLSessionConfigurationHostObject {}

#[derive(Default)]
struct NSURLSessionHostObject {
    configuration: id,
    delegate: id,
    delegate_queue: id,
    session_description: id,
}
impl HostObject for NSURLSessionHostObject {}

#[derive(Default)]
struct NSURLSessionTaskHostObject {
    /// Retained until completion/deallocation so delegate callbacks remain valid.
    session: id,
    /// `NSURLRequest *` — retained.
    original_request: id,
    /// `NSData *` supplied to an upload-task factory, if any.
    upload_body: id,
    /// The copied completion-handler block, or nil.
    completion_handler: id,
    /// `-setTaskDescription:` copy, shown in debugging tools. Purely
    /// informational, but Appodeal and other networking libraries set it.
    task_description: id,
    state: i64,
    task_identifier: u64,
    is_download: bool,
    /// Stored `NSURLResponse *` and `NSError *` for the task accessors.
    response: id,
    error: id,
    bytes_received: i64,
    bytes_sent: i64,
    receiver: Option<url_loading::HttpReceiver>,
}
impl HostObject for NSURLSessionTaskHostObject {}

/// Invoke a `void (^)(NSData *, NSURLResponse *, NSError *)` completion block.
/// Returns silently if `block` is nil or its invoke pointer is zero.
fn invoke_completion_handler(
    env: &mut crate::Environment,
    block: id,
    data: id,
    response: id,
    error: id,
) {
    if block == nil {
        return;
    }
    let block_ptr: crate::mem::MutPtr<u32> = Ptr::from_bits(block.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        log!(
            "Warning: NSURLSession completion block {:?} has NULL invoke \
             pointer; not calling.",
            block
        );
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr = Ptr::from_bits(block.to_bits()).cast_const();
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id, id, id)>>::call_from_host(
        &invoke,
        env,
        (block_arg, data, response, error),
    );
}

/// Invoke a `void (^)(NSArray *, NSArray *, NSArray *)` block, as used by
/// `-[NSURLSession getTasksWithCompletionHandler:]`.
fn invoke_tasks_handler(env: &mut crate::Environment, block: id, a: id, b: id, c: id) {
    if block == nil {
        return;
    }
    let block_ptr: crate::mem::MutPtr<u32> = Ptr::from_bits(block.to_bits());
    let invoke_addr: u32 = env.mem.read(block_ptr + BLOCK_INVOKE_WORD_OFFSET);
    if invoke_addr == 0 {
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: ConstVoidPtr = Ptr::from_bits(block.to_bits()).cast_const();
    <GuestFunction as CallFromHost<(), (ConstVoidPtr, id, id, id)>>::call_from_host(
        &invoke,
        env,
        (block_arg, a, b, c),
    );
}

#[derive(Clone, Copy)]
enum TaskKind {
    Data,
    Download,
    Upload,
}

/// Shared task-creation logic. The session and request are retained so an
/// in-flight task never outlives objects it needs for callback delivery.
fn make_task(
    env: &mut crate::Environment,
    session: id,
    request: id,
    handler: id,
    kind: TaskKind,
    upload_body: id,
) -> id {
    let task: id = match kind {
        TaskKind::Data => msg_class![env; NSURLSessionDataTask alloc],
        TaskKind::Download => msg_class![env; NSURLSessionDownloadTask alloc],
        TaskKind::Upload => msg_class![env; NSURLSessionUploadTask alloc],
    };
    let task: id = msg![env; task init];

    retain(env, session);
    retain(env, request);
    retain(env, upload_body);
    let copied_handler: id = if handler == nil {
        nil
    } else {
        msg![env; handler copy]
    };

    let identifier = {
        let state = State::get(env);
        let id_value = state.next_task_identifier;
        state.next_task_identifier += 1;
        id_value
    };

    {
        let host = env.objc.borrow_mut::<NSURLSessionTaskHostObject>(task);
        host.session = session;
        host.original_request = request;
        host.upload_body = upload_body;
        host.completion_handler = copied_handler;
        host.state = NS_URL_SESSION_TASK_STATE_SUSPENDED;
        host.task_identifier = identifier;
        host.is_download = matches!(kind, TaskKind::Download);
        host.response = nil;
        host.error = nil;
        host.bytes_received = 0;
        host.bytes_sent = 0;
        host.task_description = nil;
        host.receiver = None;
    }

    autorelease(env, task)
}

fn schedule_task_poll(env: &mut crate::Environment, task: id, delay: f64) {
    let selector = env
        .objc
        .register_host_selector("_touchHLE_pollNetwork".to_string(), &mut env.mem);
    () = msg![env; task performSelector:selector withObject:nil afterDelay:delay];
}

fn save_download_to_temporary_file(
    env: &mut crate::Environment,
    task_identifier: u64,
    bytes: &[u8],
) -> Result<id, url_loading::HttpError> {
    use crate::frameworks::foundation::ns_string::from_rust_string;
    static NEXT_DOWNLOAD_FILE: AtomicU64 = AtomicU64::new(0);

    let unique = NEXT_DOWNLOAD_FILE.fetch_add(1, Ordering::Relaxed);
    let path = env.fs.home_directory().join(format!(
        "tmp/touchHLE_download_{}_{}_{}.tmp",
        std::process::id(),
        task_identifier,
        unique
    ));
    env.fs
        .write(&path, bytes)
        .map_err(|_| url_loading::HttpError {
            code: -3000,
            description: "Could not write the downloaded file into the app's temporary directory."
                .to_string(),
        })?;
    let path_string = from_rust_string(env, path.as_str().to_string());
    autorelease(env, path_string);
    let location: id = msg_class![env; NSURL fileURLWithPath:path_string];
    Ok(location)
}

fn finish_task(env: &mut crate::Environment, task: id, result: url_loading::HttpResult) {
    let (is_download, identifier, current_state) = {
        let host = env.objc.borrow::<NSURLSessionTaskHostObject>(task);
        (host.is_download, host.task_identifier, host.state)
    };
    if current_state == NS_URL_SESSION_TASK_STATE_COMPLETED {
        return;
    }

    let (result, download_location) = match result {
        Ok(response) if is_download => {
            match save_download_to_temporary_file(env, identifier, &response.body) {
                Ok(location) => (Ok(response), location),
                Err(error) => (Err(error), nil),
            }
        }
        other => (other, nil),
    };

    let (response_object, first_result, error_object, received) = match result {
        Ok(response) => {
            let (response_object, first_result) = if is_download {
                (
                    url_loading::make_foundation_http_response(env, &response),
                    download_location,
                )
            } else {
                url_loading::make_foundation_response(env, &response)
            };
            (
                response_object,
                first_result,
                nil,
                response.body.len().min(i64::MAX as usize) as i64,
            )
        }
        Err(error) => (nil, nil, url_loading::make_foundation_error(env, &error), 0),
    };

    retain(env, response_object);
    retain(env, error_object);
    let (session, handler, previous_response, previous_error) = {
        let host = env.objc.borrow_mut::<NSURLSessionTaskHostObject>(task);
        host.state = NS_URL_SESSION_TASK_STATE_COMPLETED;
        host.receiver = None;
        host.bytes_received = received;
        let previous_response = std::mem::replace(&mut host.response, response_object);
        let previous_error = std::mem::replace(&mut host.error, error_object);
        (
            host.session,
            host.completion_handler,
            previous_response,
            previous_error,
        )
    };
    release(env, previous_response);
    release(env, previous_error);

    if handler != nil {
        invoke_completion_handler(env, handler, first_result, response_object, error_object);
        return;
    }
    let delegate: id = if session != nil {
        env.objc.borrow::<NSURLSessionHostObject>(session).delegate
    } else {
        nil
    };
    if delegate == nil {
        return;
    }
    if error_object == nil {
        if is_download {
            () = msg![env; delegate URLSession:session downloadTask:task didFinishDownloadingToURL:first_result];
        } else {
            () = msg![env; delegate URLSession:session dataTask:task didReceiveData:first_result];
        }
    }
    () = msg![env; delegate URLSession:session task:task didCompleteWithError:error_object];
}

fn start_task_network(env: &mut crate::Environment, task: id) {
    let (session, request, upload_body, existing_receiver) = {
        let host = env.objc.borrow::<NSURLSessionTaskHostObject>(task);
        (
            host.session,
            host.original_request,
            host.upload_body,
            host.receiver.is_some(),
        )
    };
    if existing_receiver {
        schedule_task_poll(env, task, 0.05);
        return;
    }

    let (additional_headers, timeout) = if session != nil {
        let configuration: id = msg![env; session configuration];
        if configuration != nil {
            (
                msg![env; configuration HTTPAdditionalHeaders],
                Some(msg![env; configuration timeoutIntervalForRequest]),
            )
        } else {
            (nil, None)
        }
    } else {
        (nil, None)
    };
    let mut request_data =
        url_loading::request_from_objc(env, request, additional_headers, timeout);
    if upload_body != nil {
        request_data = match request_data {
            Ok(mut request_data) => {
                let length: NSUInteger = msg![env; upload_body length];
                if length > 64 * 1024 * 1024 {
                    Err(url_loading::HttpError {
                        code: -1103,
                        description: "The upload body exceeds touchHLE's 64 MiB safety limit."
                            .to_string(),
                    })
                } else if length == 0 {
                    request_data.body.clear();
                    Ok(request_data)
                } else {
                    let bytes: crate::mem::ConstVoidPtr = msg![env; upload_body bytes];
                    request_data.body = env.mem.bytes_at(bytes.cast(), length).to_vec();
                    Ok(request_data)
                }
            }
            Err(error) => Err(error),
        };
    }
    let bytes_sent = request_data.as_ref().map_or(0, |request| {
        request.body.len().min(i64::MAX as usize) as i64
    });
    let receiver = url_loading::send_async(request_data, env.options.network_access);
    {
        let host = env.objc.borrow_mut::<NSURLSessionTaskHostObject>(task);
        host.bytes_sent = bytes_sent;
        host.receiver = Some(receiver);
    }
    schedule_task_poll(env, task, 0.05);
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// ===========================================================================
// NSURLSessionConfiguration
// ===========================================================================
@implementation NSURLSessionConfiguration: NSObject

+ (id)allocWithZone:(crate::objc::NSZonePtr)_zone {
    let host = Box::<NSURLSessionConfigurationHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)defaultSessionConfiguration {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    autorelease(env, new)
}

+ (id)ephemeralSessionConfiguration {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    autorelease(env, new)
}

+ (id)backgroundSessionConfigurationWithIdentifier:(id)identifier {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    retain(env, identifier);
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(new)
        .identifier = identifier;
    autorelease(env, new)
}

// Deprecated spelling kept by some apps.
+ (id)backgroundSessionConfiguration:(id)identifier {
    msg![env; this backgroundSessionConfigurationWithIdentifier:identifier]
}

- (id)init {
    let this: id = msg_super![env; this init];
    {
        let host = env
            .objc
            .borrow_mut::<NSURLSessionConfigurationHostObject>(this);
        host.allows_cellular_access = true;
        // NSURLRequestUseProtocolCachePolicy is the documented default.
        host.request_cache_policy = 0;
        // Apple defaults: 60s request timeout, 7 days resource timeout.
        host.timeout_interval_for_request = 60.0;
        host.timeout_interval_for_resource = 604800.0;
    }
    this
}

- (id)identifier {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .identifier
}

- (bool)allowsCellularAccess {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .allows_cellular_access
}
- (())setAllowsCellularAccess:(bool)value {
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .allows_cellular_access = value;
}

- (id)HTTPAdditionalHeaders {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .http_additional_headers
}
- (())setHTTPAdditionalHeaders:(id)headers {
    retain(env, headers);
    let old = env
        .objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .http_additional_headers;
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .http_additional_headers = headers;
    release(env, old);
}

- (id)URLCache {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .url_cache
}
- (())setURLCache:(id)url_cache {
    retain(env, url_cache);
    let old = env
        .objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .url_cache;
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .url_cache = url_cache;
    release(env, old);
}

- (NSUInteger)requestCachePolicy {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .request_cache_policy
}
- (())setRequestCachePolicy:(NSUInteger)policy {
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .request_cache_policy = policy;
}

- (f64)timeoutIntervalForRequest {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .timeout_interval_for_request
}
- (())setTimeoutIntervalForRequest:(f64)value {
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .timeout_interval_for_request = value;
}

- (f64)timeoutIntervalForResource {
    env.objc
        .borrow::<NSURLSessionConfigurationHostObject>(this)
        .timeout_interval_for_resource
}
- (())setTimeoutIntervalForResource:(f64)value {
    env.objc
        .borrow_mut::<NSURLSessionConfigurationHostObject>(this)
        .timeout_interval_for_resource = value;
}

- (())dealloc {
    let &NSURLSessionConfigurationHostObject {
        identifier,
        http_additional_headers,
        url_cache,
        ..
    } = env
        .objc
        .borrow::<NSURLSessionConfigurationHostObject>(this);
    release(env, identifier);
    release(env, http_additional_headers);
    release(env, url_cache);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

// ===========================================================================
// NSURLSession
// ===========================================================================
@implementation NSURLSession: NSObject

+ (id)allocWithZone:(crate::objc::NSZonePtr)_zone {
    let host = Box::<NSURLSessionHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

+ (id)sharedSession {
    let cached = State::get(env).shared_session;
    if cached != nil {
        return cached;
    }
    let config: id = msg_class![env; NSURLSessionConfiguration defaultSessionConfiguration];
    let session: id = msg![env; this sessionWithConfiguration:config];
    retain(env, session);
    State::get(env).shared_session = session;
    session
}

+ (id)sessionWithConfiguration:(id)configuration {
    msg![env; this sessionWithConfiguration:configuration delegate:nil delegateQueue:nil]
}

+ (id)sessionWithConfiguration:(id)configuration
                      delegate:(id)delegate
                 delegateQueue:(id)queue {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];

    retain(env, configuration);
    retain(env, delegate);
    retain(env, queue);
    {
        let host = env.objc.borrow_mut::<NSURLSessionHostObject>(new);
        host.configuration = configuration;
        host.delegate = delegate;
        host.delegate_queue = queue;
    }
    autorelease(env, new)
}

- (id)configuration {
    env.objc.borrow::<NSURLSessionHostObject>(this).configuration
}
- (id)delegate {
    env.objc.borrow::<NSURLSessionHostObject>(this).delegate
}
- (id)delegateQueue {
    env.objc.borrow::<NSURLSessionHostObject>(this).delegate_queue
}

- (())getTasksWithCompletionHandler:(id)handler {
    // touchHLE runs every task to completion on the guest run loop and does
    // not keep a registry of finished tasks, so the truthful answer is three
    // empty arrays. Networking libraries (Appodeal among them) call this on
    // startup to observe existing work; leaving the selector unimplemented
    // makes them throw and never finish initialising.
    let data_tasks: id = msg_class![env; NSArray array];
    let upload_tasks: id = msg_class![env; NSArray array];
    let download_tasks: id = msg_class![env; NSArray array];
    invoke_tasks_handler(env, handler, data_tasks, upload_tasks, download_tasks);
}

- (id)sessionDescription {
    env.objc.borrow::<NSURLSessionHostObject>(this).session_description
}
- (())setSessionDescription:(id)description {
    let copy: id = msg![env; description copy];
    let old = env.objc.borrow::<NSURLSessionHostObject>(this).session_description;
    env.objc.borrow_mut::<NSURLSessionHostObject>(this).session_description = copy;
    release(env, old);
}

- (())getAllTasksWithCompletionHandler:(id)handler {
    () = msg![env; this getTasksWithCompletionHandler:handler];
}

// MARK: - Task creation

- (id)dataTaskWithRequest:(id)request {
    make_task(env, this, request, nil, TaskKind::Data, nil)
}

- (id)dataTaskWithRequest:(id)request
        completionHandler:(id)handler {
    make_task(env, this, request, handler, TaskKind::Data, nil)
}

- (id)dataTaskWithURL:(id)url {
    let request: id = msg_class![env; NSURLRequest requestWithURL:url];
    make_task(env, this, request, nil, TaskKind::Data, nil)
}

- (id)dataTaskWithURL:(id)url
    completionHandler:(id)handler {
    let request: id = msg_class![env; NSURLRequest requestWithURL:url];
    make_task(env, this, request, handler, TaskKind::Data, nil)
}

- (id)downloadTaskWithRequest:(id)request {
    make_task(env, this, request, nil, TaskKind::Download, nil)
}
- (id)downloadTaskWithRequest:(id)request
            completionHandler:(id)handler {
    make_task(env, this, request, handler, TaskKind::Download, nil)
}
- (id)downloadTaskWithURL:(id)url {
    let request: id = msg_class![env; NSURLRequest requestWithURL:url];
    make_task(env, this, request, nil, TaskKind::Download, nil)
}
- (id)downloadTaskWithURL:(id)url
        completionHandler:(id)handler {
    let request: id = msg_class![env; NSURLRequest requestWithURL:url];
    make_task(env, this, request, handler, TaskKind::Download, nil)
}

- (id)uploadTaskWithRequest:(id)request fromData:(id)data {
    make_task(env, this, request, nil, TaskKind::Upload, data)
}
- (id)uploadTaskWithRequest:(id)request
                   fromData:(id)data
          completionHandler:(id)handler {
    make_task(env, this, request, handler, TaskKind::Upload, data)
}
- (id)uploadTaskWithRequest:(id)request fromFile:(id)file_url {
    let data: id = msg_class![env; NSData dataWithContentsOfURL:file_url];
    make_task(env, this, request, nil, TaskKind::Upload, data)
}
- (id)uploadTaskWithRequest:(id)request
                   fromFile:(id)file_url
          completionHandler:(id)handler {
    let data: id = msg_class![env; NSData dataWithContentsOfURL:file_url];
    make_task(env, this, request, handler, TaskKind::Upload, data)
}
- (id)uploadTaskWithStreamedRequest:(id)request {
    make_task(env, this, request, nil, TaskKind::Upload, nil)
}
- (id)uploadTaskWithStreamedRequest:(id)request
                  completionHandler:(id)handler {
    make_task(env, this, request, handler, TaskKind::Upload, nil)
}

// MARK: - Lifecycle

- (())finishTasksAndInvalidate {
    // No background tasks to drain in the stub.
}
- (())invalidateAndCancel {
    // Drop strong refs to delegate/queue as a real invalidated session does.
    let &NSURLSessionHostObject { delegate, delegate_queue, .. } =
        env.objc.borrow::<NSURLSessionHostObject>(this);
    release(env, delegate);
    release(env, delegate_queue);
    let host = env.objc.borrow_mut::<NSURLSessionHostObject>(this);
    host.delegate = nil;
    host.delegate_queue = nil;
}

- (())dealloc {
    let &NSURLSessionHostObject {
        configuration,
        delegate,
        delegate_queue,
        session_description,
    } = env.objc.borrow::<NSURLSessionHostObject>(this);
    release(env, configuration);
    release(env, delegate);
    release(env, delegate_queue);
    release(env, session_description);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

// ===========================================================================
// NSURLSessionTask (data/download/upload tasks all use this host class)
// ===========================================================================
@implementation NSURLSessionTask: NSObject

+ (id)allocWithZone:(crate::objc::NSZonePtr)_zone {
    let host = Box::<NSURLSessionTaskHostObject>::default();
    env.objc.alloc_object(this, host, &mut env.mem)
}

- (())resume {
    let state = env.objc.borrow::<NSURLSessionTaskHostObject>(this).state;
    if state == NS_URL_SESSION_TASK_STATE_COMPLETED
        || state == NS_URL_SESSION_TASK_STATE_CANCELING
        || state == NS_URL_SESSION_TASK_STATE_RUNNING
    {
        return;
    }
    env.objc
        .borrow_mut::<NSURLSessionTaskHostObject>(this)
        .state = NS_URL_SESSION_TASK_STATE_RUNNING;
    start_task_network(env, this);
}

- (())suspend {
    let host = env.objc.borrow_mut::<NSURLSessionTaskHostObject>(this);
    if host.state == NS_URL_SESSION_TASK_STATE_RUNNING {
        host.state = NS_URL_SESSION_TASK_STATE_SUSPENDED;
    }
}

- (())cancel {
    let receiver = {
        let host = env.objc.borrow_mut::<NSURLSessionTaskHostObject>(this);
        if host.state == NS_URL_SESSION_TASK_STATE_COMPLETED
            || host.state == NS_URL_SESSION_TASK_STATE_CANCELING
        {
            return;
        }
        host.state = NS_URL_SESSION_TASK_STATE_CANCELING;
        host.receiver.take()
    };
    drop(receiver);
    let selector = env
        .objc
        .register_host_selector("_touchHLE_completeCancellation".to_string(), &mut env.mem);
    () = msg![env; this performSelector:selector withObject:nil afterDelay:0.0_f64];
}

- (())_touchHLE_completeCancellation {
    if env.objc.borrow::<NSURLSessionTaskHostObject>(this).state
        != NS_URL_SESSION_TASK_STATE_CANCELING
    {
        return;
    }
    finish_task(
        env,
        this,
        Err(url_loading::HttpError {
            code: -999,
            description: "The request was cancelled.".to_string(),
        }),
    );
}

- (())_touchHLE_pollNetwork {
    let poll = {
        let host = env.objc.borrow::<NSURLSessionTaskHostObject>(this);
        if host.state != NS_URL_SESSION_TASK_STATE_RUNNING {
            return;
        }
        let Some(receiver) = host.receiver.as_ref() else {
            return;
        };
        receiver.try_recv()
    };
    match poll {
        Ok(result) => finish_task(env, this, result),
        Err(TryRecvError::Empty) => schedule_task_poll(env, this, 0.05),
        Err(TryRecvError::Disconnected) => finish_task(
            env,
            this,
            Err(url_loading::HttpError {
                code: -1005,
                description: "The HTTP worker stopped before returning a result.".to_string(),
            }),
        ),
    }
}

- (id)taskDescription {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).task_description
}
- (())setTaskDescription:(id)description {
    let copy: id = if description == nil {
        nil
    } else {
        msg![env; description copy]
    };
    let old = env
        .objc
        .borrow::<NSURLSessionTaskHostObject>(this)
        .task_description;
    env.objc
        .borrow_mut::<NSURLSessionTaskHostObject>(this)
        .task_description = copy;
    release(env, old);
}

- (i64)state {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).state
}

- (u64)taskIdentifier {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).task_identifier
}

- (id)originalRequest {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).original_request
}
- (id)currentRequest {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).original_request
}
- (id)response {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).response
}
- (id)error {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).error
}
- (i64)countOfBytesReceived {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).bytes_received
}
- (i64)countOfBytesExpectedToReceive {
    let response = env.objc.borrow::<NSURLSessionTaskHostObject>(this).response;
    if response == nil {
        -1
    } else {
        msg![env; response expectedContentLength]
    }
}
- (i64)countOfBytesSent {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).bytes_sent
}
- (i64)countOfBytesExpectedToSend {
    env.objc.borrow::<NSURLSessionTaskHostObject>(this).bytes_sent
}

- (())dealloc {
    let (
        session,
        original_request,
        upload_body,
        completion_handler,
        task_description,
        response,
        error,
    ) = {
        let host = env.objc.borrow::<NSURLSessionTaskHostObject>(this);
        (
            host.session,
            host.original_request,
            host.upload_body,
            host.completion_handler,
            host.task_description,
            host.response,
            host.error,
        )
    };
    release(env, session);
    release(env, original_request);
    release(env, upload_body);
    release(env, completion_handler);
    release(env, task_description);
    release(env, response);
    release(env, error);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

// Subclass spellings used by apps that down-cast; they all behave identically
// to NSURLSessionTask in this offline stub.
@implementation NSURLSessionDataTask: NSURLSessionTask
@end

@implementation NSURLSessionDownloadTask: NSURLSessionTask
@end

@implementation NSURLSessionUploadTask: NSURLSessionTask
@end

};
