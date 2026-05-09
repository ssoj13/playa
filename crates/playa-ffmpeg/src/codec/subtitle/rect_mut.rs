use std::{ffi::CString, ops::Deref};

use super::{Ass, Bitmap, Flags, Text, Type};
use crate::ffi::*;
use libc::c_int;

pub enum RectMut<'a> {
    None(*mut AVSubtitleRect),
    Bitmap(BitmapMut<'a>),
    Text(TextMut<'a>),
    Ass(AssMut<'a>),
}

impl<'a> RectMut<'a> {
    pub unsafe fn wrap(ptr: *mut AVSubtitleRect) -> Self {
        match Type::from(unsafe { (*ptr).type_ }) {
            Type::None => RectMut::None(ptr),
            Type::Bitmap => RectMut::Bitmap(unsafe { BitmapMut::wrap(ptr) }),
            Type::Text => RectMut::Text(unsafe { TextMut::wrap(ptr) }),
            Type::Ass => RectMut::Ass(unsafe { AssMut::wrap(ptr) }),
        }
    }

    pub unsafe fn as_ptr(&self) -> *const AVSubtitleRect {
        match *self {
            RectMut::None(ptr) => ptr as *const _,
            RectMut::Bitmap(ref b) => unsafe { b.as_ptr() },
            RectMut::Text(ref t) => unsafe { t.as_ptr() },
            RectMut::Ass(ref a) => unsafe { a.as_ptr() },
        }
    }

    pub unsafe fn as_mut_ptr(&mut self) -> *mut AVSubtitleRect {
        match *self {
            RectMut::None(ptr) => ptr,
            RectMut::Bitmap(ref mut b) => unsafe { b.as_mut_ptr() },
            RectMut::Text(ref mut t) => unsafe { t.as_mut_ptr() },
            RectMut::Ass(ref mut a) => unsafe { a.as_mut_ptr() },
        }
    }
}

impl<'a> RectMut<'a> {
    pub fn flags(&self) -> Flags {
        unsafe {
            Flags::from_bits_truncate(match *self {
                RectMut::None(ptr) => (*ptr).flags,
                RectMut::Bitmap(ref b) => (*b.as_ptr()).flags,
                RectMut::Text(ref t) => (*t.as_ptr()).flags,
                RectMut::Ass(ref a) => (*a.as_ptr()).flags,
            })
        }
    }
}

pub struct BitmapMut<'a> {
    immutable: Bitmap<'a>,
}

impl<'a> BitmapMut<'a> {
    pub unsafe fn wrap(ptr: *mut AVSubtitleRect) -> Self {
        BitmapMut { immutable: unsafe { Bitmap::wrap(ptr as *const _) } }
    }

    pub unsafe fn as_mut_ptr(&mut self) -> *mut AVSubtitleRect {
        unsafe { self.as_ptr() as *mut _ }
    }
}

impl<'a> BitmapMut<'a> {
    pub fn set_x(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).x = value as c_int;
        }
    }

    pub fn set_y(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).y = value as c_int;
        }
    }

    pub fn set_width(&mut self, value: u32) {
        unsafe {
            (*self.as_mut_ptr()).w = value as c_int;
        }
    }

    pub fn set_height(&mut self, value: u32) {
        unsafe {
            (*self.as_mut_ptr()).h = value as c_int;
        }
    }

    pub fn set_colors(&mut self, value: usize) {
        unsafe {
            (*self.as_mut_ptr()).nb_colors = value as c_int;
        }
    }
}

impl<'a> Deref for BitmapMut<'a> {
    type Target = Bitmap<'a>;

    fn deref(&self) -> &Self::Target {
        &self.immutable
    }
}

pub struct TextMut<'a> {
    immutable: Text<'a>,
}

impl<'a> TextMut<'a> {
    pub unsafe fn wrap(ptr: *mut AVSubtitleRect) -> Self {
        TextMut { immutable: unsafe { Text::wrap(ptr as *const _) } }
    }

    pub unsafe fn as_mut_ptr(&mut self) -> *mut AVSubtitleRect {
        unsafe { self.as_ptr() as *mut _ }
    }
}

impl<'a> TextMut<'a> {
    pub fn set(&mut self, value: &str) {
        let value = CString::new(value).unwrap();

        unsafe {
            (*self.as_mut_ptr()).text = av_strdup(value.as_ptr());
        }
    }
}

impl<'a> Deref for TextMut<'a> {
    type Target = Text<'a>;

    fn deref(&self) -> &Self::Target {
        &self.immutable
    }
}

pub struct AssMut<'a> {
    immutable: Ass<'a>,
}

impl<'a> AssMut<'a> {
    pub unsafe fn wrap(ptr: *mut AVSubtitleRect) -> Self {
        AssMut { immutable: unsafe { Ass::wrap(ptr) } }
    }

    pub unsafe fn as_mut_ptr(&mut self) -> *mut AVSubtitleRect {
        unsafe { self.as_ptr() as *mut _ }
    }
}

impl<'a> AssMut<'a> {
    pub fn set(&mut self, value: &str) {
        let value = CString::new(value).unwrap();

        unsafe {
            (*self.as_mut_ptr()).ass = av_strdup(value.as_ptr());
        }
    }
}

impl<'a> Deref for AssMut<'a> {
    type Target = Ass<'a>;

    fn deref(&self) -> &Self::Target {
        &self.immutable
    }
}
