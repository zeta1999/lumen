use std::convert::TryFrom;
use std::mem::transmute;

use num_bigint::BigInt;
use num_traits::Zero;

use crate::atom::{Existence, Existence::*};
use crate::exception::{self, Exception};
use crate::list::{Cons, ToList};
use crate::process::{IntoProcess, Process, TryIntoInProcess};
use crate::term::{self, Tag::*, Term};

#[macro_use]
pub mod sub;

pub mod heap;

pub enum Binary<'a> {
    Heap(&'a heap::Binary),
    Sub(&'a sub::Binary),
}

trait ByteIterator: ExactSizeIterator + DoubleEndedIterator + Iterator<Item = u8>
where
    Self: Sized,
{
    fn next_f64(&mut self) -> Option<f64> {
        match (
            self.next(),
            self.next(),
            self.next(),
            self.next(),
            self.next(),
            self.next(),
            self.next(),
            self.next(),
        ) {
            (
                Some(first_byte),
                Some(second_byte),
                Some(third_byte),
                Some(fourth_byte),
                Some(fifth_byte),
                Some(sixth_byte),
                Some(seventh_byte),
                Some(eighth_byte),
            ) => {
                let unsigned = ((first_byte as u64) << 56)
                    | ((second_byte as u64) << 48)
                    | ((third_byte as u64) << 40)
                    | ((fourth_byte as u64) << 32)
                    | ((fifth_byte as u64) << 24)
                    | ((sixth_byte as u64) << 16)
                    | ((seventh_byte as u64) << 8)
                    | (eighth_byte as u64);

                Some(unsafe { transmute(unsigned) })
            }
            _ => None,
        }
    }

    fn next_i32(&mut self) -> Option<i32> {
        match (self.next(), self.next(), self.next(), self.next()) {
            (Some(first_byte), Some(second_byte), Some(third_byte), Some(fourth_byte)) => Some(
                ((first_byte as i32) << 24)
                    | ((second_byte as i32) << 16)
                    | ((third_byte as i32) << 8)
                    | (fourth_byte as i32),
            ),
            _ => None,
        }
    }

    fn next_u16(&mut self) -> Option<u16> {
        match (self.next(), self.next()) {
            (Some(first_byte), Some(second_byte)) => {
                Some(((first_byte as u16) << 8) | (second_byte as u16))
            }
            _ => None,
        }
    }

    fn next_u32(&mut self) -> Option<u32> {
        match (self.next(), self.next(), self.next(), self.next()) {
            (Some(first_byte), Some(second_byte), Some(third_byte), Some(fourth_byte)) => Some(
                ((first_byte as u32) << 24)
                    | ((second_byte as u32) << 16)
                    | ((third_byte as u32) << 8)
                    | (fourth_byte as u32),
            ),
            _ => None,
        }
    }

    fn next_atom(&mut self, existence: Existence) -> Option<Term> {
        self.next_u16()
            .and_then(|length| self.next_byte_vec(length as usize))
            .and_then(|byte_vec| match String::from_utf8(byte_vec) {
                Ok(string) => Term::str_to_atom(&string, existence),
                Err(_) => None,
            })
    }

    fn next_binary(&mut self, process: &Process) -> Option<Term> {
        self.next_u32()
            .and_then(|length| self.next_byte_vec(length as usize))
            .map(|byte_vec| Term::slice_to_binary(byte_vec.as_slice(), &process))
    }

    fn next_bit_binary(&mut self, process: &Process) -> Option<Term> {
        self.next_u32().and_then(|binary_byte_count| {
            self.next().and_then(|bit_count| {
                self.next_byte_vec(binary_byte_count as usize)
                    .map(|byte_vec| {
                        let original = Term::slice_to_binary(byte_vec.as_slice(), &process);

                        Term::subbinary(
                            original,
                            0,
                            0,
                            (binary_byte_count - 1) as usize,
                            bit_count,
                            &process,
                        )
                    })
            })
        })
    }

    fn next_byte_list(&mut self, process: &Process) -> Option<Term> {
        self.next_u16()
            .and_then(|length| self.next_byte_vec(length as usize))
            .map(|byte_vec| {
                byte_vec.iter().rfold(Term::EMPTY_LIST, |acc, element| {
                    Term::cons(element.into_process(&process), acc, &process)
                })
            })
    }

    fn next_byte_vec(&mut self, length: usize) -> Option<Vec<u8>> {
        let mut byte_vec: Vec<u8> = Vec::with_capacity(length);

        for _ in 0..length {
            match self.next() {
                Some(byte) => byte_vec.push(byte),
                None => break,
            }
        }

        if byte_vec.len() == length {
            Some(byte_vec)
        } else {
            None
        }
    }

    fn next_small_integer(&mut self) -> Option<Term> {
        self.next().map(|byte| byte.into())
    }

    fn next_integer(&mut self, process: &Process) -> Option<Term> {
        self.next_i32()
            .map(|integer| integer.into_process(&process))
    }

    fn next_list(&mut self, existence: Existence, process: &Process) -> Option<Term> {
        self.next_u32()
            .and_then(|element_count| self.next_terms(existence, element_count as usize, &process))
            .and_then(|element_vec| {
                self.next_term(existence, &process).map(|tail_term| {
                    element_vec.iter().rfold(tail_term, |acc, element| {
                        Term::cons(*element, acc, &process)
                    })
                })
            })
    }

    fn next_new_float(&mut self, process: &Process) -> Option<Term> {
        self.next_f64().map(|inner| inner.into_process(&process))
    }

    fn next_small_atom_utf8(&mut self, existence: Existence) -> Option<Term> {
        self.next()
            .and_then(|length| self.next_byte_vec(length as usize))
            .and_then(|byte_vec| match String::from_utf8(byte_vec) {
                Ok(string) => Term::str_to_atom(&string, existence),
                Err(_) => None,
            })
    }

    fn next_small_big_integer(&mut self, process: &Process) -> Option<Term> {
        self.next().and_then(|count| {
            self.next().and_then(|sign| {
                let mut big_int: BigInt = Zero::zero();
                let mut truncated = false;

                for _ in 0..count {
                    match self.next() {
                        Some(byte) => {
                            let byte_big_int: BigInt = byte.into();
                            big_int = (big_int << 8) | (byte_big_int)
                        }
                        None => {
                            truncated = true;
                            break;
                        }
                    }
                }

                if truncated {
                    None
                } else {
                    let signed_big_int = if sign == 0 { big_int } else { -1 * big_int };

                    Some(signed_big_int.into_process(&process))
                }
            })
        })
    }

    fn next_small_tuple(&mut self, existence: Existence, process: &Process) -> Option<Term> {
        self.next()
            .and_then(|arity| self.next_terms(existence, arity as usize, &process))
            .map(|element_vec| Term::slice_to_tuple(element_vec.as_slice(), &process))
    }

    fn next_term(&mut self, existence: Existence, process: &Process) -> Option<Term> {
        match self.next() {
            Some(tag_byte) => match tag_byte.try_into_in_process(&process) {
                Ok(tag) => {
                    use crate::term::external_format::Tag::*;

                    match tag {
                        Atom => self.next_atom(existence),
                        Binary => self.next_binary(&process),
                        BitBinary => self.next_bit_binary(&process),
                        ByteList => self.next_byte_list(&process),
                        EmptyList => Some(Term::EMPTY_LIST),
                        Integer => self.next_integer(&process),
                        List => self.next_list(existence, &process),
                        NewFloat => self.next_new_float(&process),
                        SmallAtomUTF8 => self.next_small_atom_utf8(existence),
                        SmallBigInteger => self.next_small_big_integer(&process),
                        SmallInteger => self.next_small_integer(),
                        SmallTuple => self.next_small_tuple(existence, &process),
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    fn next_terms(
        &mut self,
        existence: Existence,
        count: usize,
        process: &Process,
    ) -> Option<Vec<Term>> {
        let mut element_vec: Vec<Term> = Vec::with_capacity(count);

        for _ in 0..count {
            match self.next_term(existence, &process) {
                Some(term) => element_vec.push(term),
                None => break,
            }
        }

        if element_vec.len() == count {
            Some(element_vec)
        } else {
            None
        }
    }

    fn next_versioned_term(&mut self, existence: Existence, process: &Process) -> Option<Term> {
        match self.next() {
            Some(term::external_format::VERSION_NUMBER) => self.next_term(existence, &process),
            Some(version) => panic!("Unknown version number ({})", version),
            None => None,
        }
    }

    fn part_range(
        &mut self,
        PartRange {
            byte_offset,
            byte_count,
        }: PartRange,
    ) -> &mut Self {
        // skip byte_offset
        for _ in 0..byte_offset {
            self.next();
        }

        for _ in byte_count..self.len() {
            self.next_back();
        }

        self
    }
}

pub trait Part<'a, S, L, T> {
    fn part(&'a self, start: S, length: L, process: &Process) -> Result<T, Exception>;
}

pub struct PartRange {
    byte_offset: usize,
    byte_count: usize,
}

fn start_length_to_part_range(
    start: usize,
    length: isize,
    available_byte_count: usize,
) -> Result<PartRange, Exception> {
    if length >= 0 {
        let non_negative_length = length as usize;

        if (start < available_byte_count) & (start + non_negative_length <= available_byte_count) {
            Ok(PartRange {
                byte_offset: start,
                byte_count: non_negative_length,
            })
        } else {
            Err(badarg!())
        }
    } else {
        let start_isize = start as isize;

        if (start <= available_byte_count) & (0 <= start_isize + length) {
            let byte_offset = (start_isize + length) as usize;
            let byte_count = (-length) as usize;

            Ok(PartRange {
                byte_offset,
                byte_count,
            })
        } else {
            Err(badarg!())
        }
    }
}

fn part_range_to_list<T: ByteIterator>(
    mut byte_iterator: T,
    part_range: PartRange,
    process: &Process,
) -> Term {
    byte_iterator.part_range(part_range).to_list(&process)
}

pub trait PartToList<S, L> {
    fn part_to_list(&self, start: S, length: L, process: &Process) -> exception::Result;
}

pub trait ToTerm {
    fn to_term(&self, options: ToTermOptions, process: &Process) -> exception::Result;
}

pub struct ToTermOptions {
    pub existence: Existence,
    pub used: bool,
}

impl ToTermOptions {
    fn put_option_term(&mut self, option: Term) -> Result<&ToTermOptions, Exception> {
        match option.tag() {
            Atom => {
                let option_string = unsafe { option.atom_to_string() };

                match option_string.as_ref().as_ref() {
                    "safe" => {
                        self.existence = Exists;

                        Ok(self)
                    }
                    "used" => {
                        self.used = true;

                        Ok(self)
                    }
                    _ => Err(badarg!()),
                }
            }
            _ => Err(badarg!()),
        }
    }
}

impl TryFrom<Term> for ToTermOptions {
    type Error = Exception;

    fn try_from(term: Term) -> Result<ToTermOptions, Exception> {
        let mut options: ToTermOptions = Default::default();
        let mut options_term = term;

        loop {
            match options_term.tag() {
                EmptyList => return Ok(options),
                List => {
                    let cons: &Cons = unsafe { options_term.as_ref_cons_unchecked() };

                    options.put_option_term(cons.head())?;
                    options_term = cons.tail();

                    continue;
                }
                _ => return Err(badarg!()),
            };
        }
    }
}

impl Default for ToTermOptions {
    fn default() -> ToTermOptions {
        ToTermOptions {
            existence: DoNotCare,
            used: false,
        }
    }
}
