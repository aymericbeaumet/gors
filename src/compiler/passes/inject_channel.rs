use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    // Check if any channel operations are used by scanning for gors_channel references
    let mut detector = ChannelDetector { found: false };
    detector.visit_file_mut(file);

    if detector.found {
        // Inject the gors_channel module at the top of the file
        let channel_mod: syn::Item = syn::parse_quote! {
            mod gors_channel {
                use std::sync::{Arc, Mutex, Condvar};
                use std::collections::VecDeque;

                struct Inner<T> {
                    buffer: VecDeque<T>,
                    capacity: usize,
                    closed: bool,
                    senders: usize,
                }

                pub struct GoChan<T> {
                    inner: Arc<(Mutex<Inner<T>>, Condvar, Condvar)>,
                }

                impl<T> Clone for GoChan<T> {
                    fn clone(&self) -> Self {
                        let mut lock = self.inner.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        lock.senders += 1;
                        drop(lock);
                        GoChan {
                            inner: Arc::clone(&self.inner),
                        }
                    }
                }

                impl<T> GoChan<T> {
                    pub fn new(capacity: usize) -> Self {
                        GoChan {
                            inner: Arc::new((
                                Mutex::new(Inner {
                                    buffer: VecDeque::new(),
                                    capacity,
                                    closed: false,
                                    senders: 1,
                                }),
                                Condvar::new(), // notify receivers
                                Condvar::new(), // notify senders
                            )),
                        }
                    }

                    pub fn send(&self, value: T) {
                        let (ref mutex, ref recv_cvar, ref send_cvar) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if inner.capacity > 0 {
                            // Buffered: wait until there's room
                            while inner.buffer.len() >= inner.capacity && !inner.closed {
                                inner = send_cvar.wait(inner).unwrap_or_else(|poisoned| poisoned.into_inner());
                            }
                        } else {
                            // Unbuffered: wait until buffer is empty (previous value consumed)
                            while !inner.buffer.is_empty() && !inner.closed {
                                inner = send_cvar.wait(inner).unwrap_or_else(|poisoned| poisoned.into_inner());
                            }
                        }
                        if !inner.closed {
                            inner.buffer.push_back(value);
                            recv_cvar.notify_one();
                        }
                    }

                    pub fn recv(&self) -> T where T: Default {
                        let (ref mutex, ref recv_cvar, ref send_cvar) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        while inner.buffer.is_empty() && !inner.closed {
                            inner = recv_cvar.wait(inner).unwrap_or_else(|poisoned| poisoned.into_inner());
                        }
                        if let Some(val) = inner.buffer.pop_front() {
                            send_cvar.notify_one();
                            val
                        } else {
                            T::default()
                        }
                    }

                    pub fn recv_with_ok(&self) -> (T, bool) where T: Default {
                        let (ref mutex, ref recv_cvar, ref send_cvar) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        while inner.buffer.is_empty() && !inner.closed {
                            inner = recv_cvar.wait(inner).unwrap_or_else(|poisoned| poisoned.into_inner());
                        }
                        if let Some(val) = inner.buffer.pop_front() {
                            send_cvar.notify_one();
                            (val, true)
                        } else {
                            (T::default(), false)
                        }
                    }

                    pub fn try_recv(&self) -> Result<T, ()> {
                        let (ref mutex, _, ref send_cvar) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if let Some(val) = inner.buffer.pop_front() {
                            send_cvar.notify_one();
                            Ok(val)
                        } else {
                            Err(())
                        }
                    }

                    pub fn try_send(&self, value: T) -> Result<(), T> {
                        let (ref mutex, ref recv_cvar, _) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if inner.closed {
                            return Err(value);
                        }
                        if inner.capacity > 0 {
                            if inner.buffer.len() < inner.capacity {
                                inner.buffer.push_back(value);
                                recv_cvar.notify_one();
                                Ok(())
                            } else {
                                Err(value)
                            }
                        } else {
                            if inner.buffer.is_empty() {
                                inner.buffer.push_back(value);
                                recv_cvar.notify_one();
                                Ok(())
                            } else {
                                Err(value)
                            }
                        }
                    }

                    pub fn close(&self) {
                        let (ref mutex, ref recv_cvar, ref send_cvar) = *self.inner;
                        let mut inner = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        inner.closed = true;
                        recv_cvar.notify_all();
                        send_cvar.notify_all();
                    }
                }

                pub struct GoChanIter<T>(GoChan<T>);
                impl<T: Default> Iterator for GoChanIter<T> {
                    type Item = T;
                    fn next(&mut self) -> Option<T> {
                        let (val, ok) = self.0.recv_with_ok();
                        if ok { Some(val) } else { None }
                    }
                }
                impl<T: Default> IntoIterator for GoChan<T> {
                    type Item = T;
                    type IntoIter = GoChanIter<T>;
                    fn into_iter(self) -> Self::IntoIter { GoChanIter(self) }
                }

                pub fn make_chan<T>(capacity: usize) -> GoChan<T> {
                    GoChan::new(capacity)
                }
            }
        };
        file.items.insert(0, channel_mod);

        // Now rewrite all ::gors_channel:: paths to just gors_channel::
        RewritePaths.visit_file_mut(file);
    }
}

/// Detects whether any `gors_channel` references exist in the AST.
struct ChannelDetector {
    found: bool,
}

impl VisitMut for ChannelDetector {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        for segment in &path.segments {
            if segment.ident == "gors_channel" {
                self.found = true;
                return;
            }
        }
        visit_mut::visit_path_mut(self, path);
    }
}

/// Rewrite `::gors_channel::...` to `gors_channel::...` so it references the
/// injected module rather than an external crate.
struct RewritePaths;

impl VisitMut for RewritePaths {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if path.leading_colon.is_some() {
            if let Some(first) = path.segments.first() {
                if first.ident == "gors_channel" {
                    path.leading_colon = None;
                }
            }
        }
        visit_mut::visit_path_mut(self, path);
    }
}
