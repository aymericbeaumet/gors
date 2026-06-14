use std::collections::HashSet;

pub(super) const IMPORT_PATH: &str = "runtime";
const CALLERS_FUNC: &str = "Callers";
const CALLERS_FRAMES_FUNC: &str = "CallersFrames";
const FRAME_TYPE: &str = "Frame";
const FRAMES_TYPE: &str = "Frames";
const GOMAXPROCS_FUNC: &str = "GOMAXPROCS";
const GOARCH_FUNC: &str = "GOARCH";
const GOROOT_FUNC: &str = "GOROOT";
const GOOS_FUNC: &str = "GOOS";
const STRINGER_TRAIT: &str = "stringer";

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let roots = roots?;
    if roots.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    if roots.contains(CALLERS_FUNC) {
        items.push(syn::parse_quote! {
            pub fn Callers(mut skip: isize, mut pc: &mut [usize]) -> isize {
                0
            }
        });
    }
    if needs_frames(roots) {
        items.extend(frames_items());
    }
    if roots.contains(GOMAXPROCS_FUNC) {
        items.push(syn::parse_quote! {
            pub fn GOMAXPROCS(mut n: isize) -> isize {
                let current = std::thread::available_parallelism()
                    .map(|parallelism| parallelism.get() as isize)
                    .unwrap_or(1)
                    .max(1);
                if n < 1 {
                    return current;
                }
                current
            }
        });
    }
    if roots.contains(GOARCH_FUNC) {
        items.push(syn::parse_quote! {
            pub fn GOARCH() -> String {
                std::env::consts::ARCH.to_string()
            }
        });
    }
    if roots.contains(GOROOT_FUNC) {
        items.push(syn::parse_quote! {
            pub fn GOROOT() -> String {
                option_env!("GORS_BUILT_GO_SDK_PATH")
                    .unwrap_or("")
                    .to_string()
            }
        });
    }
    if roots.contains(GOOS_FUNC) {
        items.push(syn::parse_quote! {
            pub fn GOOS() -> String {
                std::env::consts::OS.to_string()
            }
        });
    }
    if roots.contains(STRINGER_TRAIT) {
        items.push(syn::parse_quote! {
            pub trait stringer: Send + Sync {
                fn String(&mut self) -> String;
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                fn __gors_interface_key(&self) -> crate::builtin::GorsInterfaceKey;
                fn __gors_clone_box(&self) -> Box<dyn stringer>;
            }
        });
    }

    (!items.is_empty()).then(|| super::super::item_mod_for(import_path, items))
}

fn needs_frames(roots: &HashSet<String>) -> bool {
    roots.contains(CALLERS_FRAMES_FUNC)
        || roots.contains(FRAME_TYPE)
        || roots.contains(FRAMES_TYPE)
        || roots
            .iter()
            .any(|root| root == "Frames::Next" || root == "Frame::clone" || root == "Frames::clone")
}

fn frames_items() -> Vec<syn::Item> {
    vec![
        syn::parse_quote! {
            #[derive(Clone, Default)]
            #[repr(C)]
            pub struct Func;
        },
        syn::parse_quote! {
            #[derive(Clone, Default)]
            #[repr(C)]
            pub struct Frame {
                pub PC: usize,
                pub Func: crate::builtin::GorsPtr<Func>,
                pub Function: String,
                pub File: String,
                pub Line: isize,
                pub Entry: usize,
            }
        },
        syn::parse_quote! {
            #[derive(Clone, Default)]
            #[repr(C)]
            pub struct Frames {
                frames: Vec<Frame>,
                next: isize,
            }
        },
        syn::parse_quote! {
            pub fn CallersFrames(mut callers: Vec<usize>) -> crate::builtin::GorsPtr<Frames> {
                crate::builtin::GorsPtr::new(Frames {
                    frames: Vec::new(),
                    next: 0,
                })
            }
        },
        syn::parse_quote! {
            impl Frames {
                pub fn Next(mut ci: crate::builtin::GorsPtr<Self>) -> (Frame, bool) {
                    let mut guard = ci.lock().unwrap();
                    if guard.next < 0 || guard.next >= guard.frames.len() as isize {
                        return (Frame::default(), false);
                    }
                    let frame = guard.frames[guard.next as usize].clone();
                    guard.next += 1;
                    let more = guard.next < guard.frames.len() as isize;
                    (frame, more)
                }
            }
        },
    ]
}
