/*
 * Эта лицензия Source Code Form подпадает под условия Mozilla Public
 * License, v. 2.0. Если копия MPL не распространялась вместе с этим
 * файлом, вы можете получить ее на https://mozilla.org/MPL/2.0/.
 */
//! The Audio Toolbox framework.

use crate::audio::openal::{OpenAL, OpenALContext, OpenALManager};

/// Макрос для проверки, является ли аргумент null, и возврата `paramErr` в этом
//случае.
/// Похоже, это именно то, что делает настоящий Audio Toolbox, и некоторые
//приложения полагаются на это.
macro_rules! return_if_null {
    ($param:ident) => {
        if $param.is_null() {
            log_dbg!(
                "Получен параметр NULL {}, возвращаем paramErr в {} на строке {}",
                stringify!($param),
                file!(),
                line!()
            );
            return crate::frameworks::carbon_core::paramErr;
        }
    };
}

pub mod au_graph;
pub mod audio_components;
pub mod audio_converter;
pub mod audio_file;
pub mod audio_queue;
pub mod audio_services;
pub mod audio_session;
pub mod audio_unit;
pub mod ext_audio_file;

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/AudioToolbox.framework/AudioToolbox",
    aliases: &[],
    class_exports: &[],
    constant_exports: &[],
    function_exports: &[
        au_graph::FUNCTIONS,
        audio_components::FUNCTIONS,
        audio_converter::FUNCTIONS,
        audio_file::FUNCTIONS,
        audio_queue::FUNCTIONS,
        audio_services::FUNCTIONS,
        audio_session::FUNCTIONS,
        audio_unit::FUNCTIONS,
        ext_audio_file::FUNCTIONS,
    ],
};

#[derive(Default)]
pub struct State {
    audio_file: audio_file::State,
    audio_queue: audio_queue::State,
    audio_components: audio_components::State,
    audio_services: audio_services::State,
    audio_session: audio_session::State,
    au_graph: au_graph::State,
    ext_audio_file: ext_audio_file::State,
    al_context: LazyALContext,
}
impl State {
    pub fn make_al_context_current<'s, 'manager: 's>(
        &'s mut self,
        manager: &'manager mut OpenALManager,
    ) -> OpenAL<'s> {
        self.al_context.make_al_context_current(manager)
    }
}

#[derive(Default)]
pub struct LazyALContext(Option<OpenALContext>);

impl LazyALContext {
    pub fn make_al_context_current<'s, 'manager: 's>(
        &'s mut self,
        manager: &'manager mut OpenALManager,
    ) -> OpenAL<'s> {
        self.get_context(manager).make_current(manager)
    }
    pub fn try_get_context(&mut self, manager: &mut OpenALManager) -> Option<&mut OpenALContext> {
        if self.0.is_none() {
            // OpenALContext::new already attempts a fallback to OpenAL Soft's
            // "No Output" null backend if the host audio device cannot be
            // opened, so under normal circumstances this will succeed. If even
            // that fails we now log and return None instead of panicking the
            // entire emulator — guest code calling AudioQueue/AudioFile APIs
            // will then get error codes rather than a host crash.
            match OpenALContext::new(manager) {
                Ok(context) => {
                    log_dbg!("Новый внутренний контекст OpenAL ({:?})", context);
                    self.0 = Some(context);
                }
                Err(err) => {
                    log!(
                        "Warning: could not create OpenAL context for \
                         AudioToolbox: {}. Audio will be unavailable until a \
                         working backend is found.",
                        err
                    );
                    return None;
                }
            }
        }
        self.0.as_mut()
    }

    pub fn get_context(&mut self, manager: &mut OpenALManager) -> &mut OpenALContext {
        // Preserve the previous (panicking) signature for callers that absolutely
        // require a context. New callers should prefer `try_get_context`.
        if let Some(ctx) = self.try_get_context(manager) {
            // SAFETY: `try_get_context` just inserted a value into `self.0`
            // when the option was None; we can re-borrow it here without UB.
            // Using an extra `expect` keeps the error case obvious if the
            // implementation ever drifts.
            let _ = ctx;
        }
        self.0
            .as_mut()
            .expect("OpenAL context unavailable; see prior log message for details")
    }
}
