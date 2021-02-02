# Lexer IO Perf

This is a test of various methods of buffering input when reading files into a lexical analyzer.

Methods tested:
+ Direct read from stream, one byte at a time
+ Buffered read from stream, 1kb at a time
+ Buffered read from stream, 1mb at a time
+ Memory mapped read
+ Read all to string (utf-8 validated)
+ Read all to byte vector

Each method was tested on a ~300kb source file, and a ~3mb source file.


## Methodology

Two test files were generated using regex generators. The content of these files looks like this:

```
jvvxrTLUJNA7r_3k2r3VhYqrKcj9hwcKaZgI9uS19w7nE8J0kdh0HVjTv2XwUC5Zx1hkaQfEW_92DjFvQCpbtEz1u
S2BQBBcCT4MM_ghekImB8
8894
I_1i5U9TqOA7Jea7l1TvajwB4OuykRWtotUSGlFTwovHOKfaigrDC_R
668579675
```

Variably repeating for 10k and 110k lines, respectively. A generic lexical analyzer is constructed using a single byte of lookahead:
```rs
pub struct Lexer<I: Iterator<Item = u8>> {
	inner: Peekable<I>
}
```

The lexer discards whitespace, and produces a simple token sum type:
```rs
#[derive(Debug)]
pub enum Token {
	Identifier(String),
	Number(i64)
}
```

Tokens are generated via an Iterator implementation:
```rs
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
```

For each kind of test, where necessary an intermediate iterator is constructed:


### ByteReader

This is simply a wrapper to convert an `io::Read` into an `Iterator`

```rs
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
```


### BufferedReader

This takes a reference to a mutable slice, and refills it with bytes on demand

```rs
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
```


### MMapReader

This calls `mmap` on a file to generate a simple memory mapped view of it, then simply increments the base pointer. This will work on Unix systems, but an alternative implementation would be required for Windows

```rs
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
```


## Results

Results are sorted from slowest to fastest

### ~300kb
| Method              | Average time (ns) | Max deviation |
|---------------------|-------------------|---------------|
| byte_reader         | 81,016,095        | 3,666,607     |
| buffered_reader_1kb | 3,048,091         | 196,842       |
| buffered_reader_1mb | 2,885,936         | 277,377       |
| read_to_vec         | 2,664,660         | 82,730        |
| read_to_string      | 2,663,513         | 28,455        |
| mmap                | 2,577,370         | 36,680        |

### ~3mb
| Method              | Average time (ns) | Max deviation |
|---------------------|-------------------|---------------|
| byte_reader         | 884,473,603       | 24,798,573    |
| buffered_reader_1kb | 36,473,766        | 1,860,908     |
| buffered_reader_1mb | 35,053,802        | 1,157,109     |
| read_to_string      | 32,273,945        | 465,271       |
| read_to_vec         | 32,116,772        | 603,392       |
| mmap                | 30,200,108        | 569,450       |

## Takeaways

As with any benchmark, you should take all of this with a grain of salt and perform more focused, realistic tests when you are able

+ As long as you are using some kind of buffering for your input stream, you're doing okay. Reading one byte a time simply isn't feasible and the kernel won't save you here

+ The best bang-for-buck here is reading the entire input into a string or vector, before lexing. This has the least complexity of any solution, and second best performance. Better performance could be extracted by reusing the buffer for each subsequent reading operation

+ `mmap` has great performance and stability, but it will require building a multi-platform abstraction

+ Comparing `read_to_string` and `read_to_vec` demonstrates that performing utf-8 validation on the data once it is in memory is extremely trivial, and has less impact on the final numbers than basic noise and initial machine state