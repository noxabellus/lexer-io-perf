#![allow(soft_unstable)]
#![feature(test)]

use std::{ fs::File, io::Read, iter::Peekable, marker::PhantomData, ptr };


#[derive(Debug)]
pub enum Token {
	Identifier(String),
	Number(i64)
}

pub struct Lexer<I: Iterator<Item = u8>> {
	inner: Peekable<I>
}

impl<I: Iterator<Item = u8>> Lexer<I> {
	pub fn new (inner: I) -> Self {
		Self {
			inner: inner.peekable(),
		}
	}
}

impl<I: Iterator<Item = u8>> Iterator for Lexer<I> {
	type Item = Token;

	fn next (&mut self) -> Option<Token> {
		if let Some(&ch) = self.inner.peek() {
			match ch {
				x if x.is_ascii_alphabetic() || x == b'_' => {
					let mut s = vec![x];
					self.inner.next();

					while let Some(&ch) = self.inner.peek() {
						if ch.is_ascii_alphanumeric()
						|| ch == b'_' {
							s.push(ch);
							self.inner.next();
						} else {
							break
						}
					}

					Some(Token::Identifier(unsafe { String::from_utf8_unchecked(s) }))
				}

				x if x.is_ascii_digit() => {
					let mut s = vec![x];
					self.inner.next();

					while let Some(&ch) = self.inner.peek() {
						if ch.is_ascii_digit() {
							s.push(ch);
							self.inner.next();
						} else {
							break
						}
					}

					Some(Token::Number(unsafe { String::from_utf8_unchecked(s) }.parse().unwrap()))
				}

				x if x.is_ascii_whitespace() => {
					self.inner.next();
					self.next()
				}

				_ => None
			}
		} else {
			None
		}
	}
}



pub struct ByteReader<R: Read> {
	inner: R
}

impl<R: Read> ByteReader<R> {
	pub fn new (inner: R) -> Self {
		Self {
			inner
		}
	}
}

impl<R: Read> Iterator for ByteReader<R> {
	type Item = u8;

	fn next (&mut self) -> Option<u8> {
		let mut buf = [0u8; 1];
		match self.inner.read_exact(&mut buf) {
			Ok(_) => Some(unsafe { *buf.get_unchecked(0) }),
			Err(_) => None
		}
	}
}




pub fn make_box (length: usize) -> Box<[u8]> {
	let mut mem = Vec::with_capacity(length);
	unsafe { mem.set_len(length) };
	mem.into_boxed_slice()
}

pub struct BufferedReader<'b, R: Read> {
	inner: R,
	buffer: &'b mut [u8],
	offset: usize,
	remainder: usize,
}

impl<'b, R: Read> BufferedReader<'b, R> {
	pub fn new (inner: R, buffer: &'b mut [u8]) -> Self {
		Self {
			inner,
			buffer,
			offset: 0,
			remainder: 0,
		}
	}

	fn refill_buffer (&mut self) -> Option<()> {
		match self.inner.read(self.buffer.as_mut()) {
			Err(_) => { None }
			Ok(n) if n == 0 => { None }
			Ok(n) => { self.offset = 0; self.remainder = n; Some(()) }
		}
	}
}

impl<'b, R: Read> Iterator for BufferedReader<'b, R> {
	type Item = u8;

	fn next (&mut self) -> Option<u8> {
		if self.offset == self.remainder {
			self.refill_buffer()?;
		}

		let offset = self.offset;

		self.offset += 1;

		Some(unsafe { *self.buffer.get_unchecked(offset) })
	}
}



pub struct MMapReader<'f> {
	base: *const u8,
	len: usize,

	ptr: *const u8,
	end: *const u8,

	f: PhantomData<&'f mut File>
}

impl<'f> MMapReader<'f> {
	pub fn new (file: &'f mut File) -> Self {
		use std::os::unix::io::AsRawFd;

		let fd = file.as_raw_fd();
		let len = file.metadata().unwrap().len() as usize;

		unsafe {
			let ptr = libc::mmap(
				ptr::null_mut(),
				len,
				libc::PROT_READ,
				libc::MAP_SHARED,
				fd,
				0
			) as *const u8;

			let end = ptr.add(len);

			Self {
				base: ptr, len,
				ptr, end,
				f: PhantomData
			}
		}
	}
}

impl<'f> Drop for MMapReader<'f> {
	fn drop (&mut self) {
		unsafe { libc::munmap(self.base as *mut _, self.len); }
	}
}

impl<'f> Iterator for MMapReader<'f> {
	type Item = u8;
	fn next (&mut self) -> Option<u8> {
		if self.ptr < self.end {
			unsafe {
				let out = *self.ptr;
				self.ptr = self.ptr.add(1);
				Some(out)
			}
		} else {
			None
		}
	}
}



#[cfg(test)]
mod tests {
	use super::*;

	extern crate test;

	const TEST_FILE: &str = "./test_300kb.src";
	const EXPECTED_ITEMS: usize = 10_000;

	const KB: usize = 1024;
	const MB: usize = KB * 1024;

	#[inline(always)]
	fn finalize<I: Iterator<Item = Token>> (it: I) {
		let v: Vec<_> = it.collect();

		assert_eq!(v.len(), EXPECTED_ITEMS);

		test::black_box(v);
	}


	#[test]
	fn test_byte_reader () {
		let f = File::open(TEST_FILE).unwrap();
		let a = ByteReader::new(f);
		let b = Lexer::new(a);

		finalize(b)
	}

	fn body_buffered_reader (buffer: &mut [u8]) {
		let f = File::open(TEST_FILE).unwrap();
		let a = BufferedReader::new(f, buffer);
		let b = Lexer::new(a);

		finalize(b)
	}

	#[test]
	fn test_buffered_reader_1kb () {
		let mut buffer = make_box(KB);
		body_buffered_reader(&mut buffer)
	}

	#[test]
	fn test_buffered_reader_1mb () {
		let mut buffer = make_box(MB);
		body_buffered_reader(&mut buffer)
	}

	#[test]
	fn test_read_to_string () {
		let mut f = File::open(TEST_FILE).unwrap();
		let mut s = String::new();
		f.read_to_string(&mut s).unwrap();
		let a = s.into_bytes().into_iter();
		let b = Lexer::new(a);

		finalize(b)
	}

	#[test]
	fn test_read_to_vec () {
		let mut f = File::open(TEST_FILE).unwrap();
		let mut v = Vec::new();
		f.read_to_end(&mut v).unwrap();
		let a = v.into_iter();
		let b = Lexer::new(a);

		finalize(b)
	}

	#[test]
	fn test_mmap () {
		let mut f = File::open(TEST_FILE).unwrap();
		let a = MMapReader::new(&mut f);
		let b = Lexer::new(a);

		finalize(b)
	}



	#[bench]
	fn bench_byte_reader (b: &mut test::Bencher) {
		b.iter(test_byte_reader)
	}

	#[bench]
	fn bench_buffered_reader_1kb (b: &mut test::Bencher) {
		let mut buffer = make_box(KB);
		b.iter(|| body_buffered_reader(&mut buffer))
	}

	#[bench]
	fn bench_buffered_reader_1mb (b: &mut test::Bencher) {
		let mut buffer = make_box(MB);
		b.iter(|| body_buffered_reader(&mut buffer))
	}

	#[bench]
	fn bench_read_to_string (b: &mut test::Bencher) {
		b.iter(test_read_to_string)
	}

	#[bench]
	fn bench_read_to_vec (b: &mut test::Bencher) {
		b.iter(test_read_to_vec)
	}

	#[bench]
	fn bench_mmap (b: &mut test::Bencher) {
		b.iter(test_mmap)
	}
}
