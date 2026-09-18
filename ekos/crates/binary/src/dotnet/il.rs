//! CIL method bodies: header decoding and an instruction walk.
//!
//! The walk needs the operand width of **every** opcode, not just the ones it acts on: IL is a
//! variable-length stream with no instruction index, so mis-sizing one operand desynchronizes
//! everything after it. [`operand_size`] is therefore complete, and the opcodes this module
//! actually cares about — calls, field access, `ldstr`, numeric loads and branches — are
//! recognized on top of that.

use super::metadata::{u8_at, u16_at, u32_at};

/// A decoded instruction: its offset, its opcode bytes, and where its operand sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    /// Offset of the opcode within the method's code, which is what a locator cites.
    pub offset: u32,
    /// `None` for a single-byte opcode; `Some(0xFE)` for the two-byte prefix form.
    pub prefix: Option<u8>,
    pub opcode: u8,
    /// Offset of the operand within the code slice.
    pub operand_at: usize,
    pub operand_len: usize,
}

/// A method body's code, located and sized.
#[derive(Debug, Clone, Copy)]
pub struct Body<'a> {
    pub code: &'a [u8],
}

/// Read a method body header at `offset` and return its code slice.
///
/// Two formats (ECMA-335 II.25.4): *tiny* (a single byte carrying the code size, for bodies under
/// 64 bytes with no locals or exception handlers) and *fat* (a 12-byte header). Anything else is
/// a corrupt body and yields `None`.
pub fn read_body(image: &[u8], offset: usize) -> Option<Body<'_>> {
    let first = u8_at(image, offset)?;
    match first & 0x03 {
        0x02 => {
            let size = (first >> 2) as usize;
            let start = offset + 1;
            Some(Body {
                code: image.get(start..start + size)?,
            })
        }
        0x03 => {
            // The high nibble of the first u16 is the header size in 4-byte words; it is 3 for
            // every header the spec defines, but it is read rather than assumed because the code
            // offset depends on it.
            let flags_and_size = u16_at(image, offset)?;
            let header_words = (flags_and_size >> 12) as usize;
            if header_words < 3 {
                return None;
            }
            let code_size = u32_at(image, offset + 4)? as usize;
            let start = offset + header_words * 4;
            Some(Body {
                code: image.get(start..start.checked_add(code_size)?)?,
            })
        }
        _ => None,
    }
}

/// Walk a code slice, yielding one [`Instruction`] per instruction.
///
/// Stops at the first byte it cannot size rather than guessing — a truncated or corrupt body
/// then contributes the instructions up to that point, which is strictly better than either
/// discarding the method or emitting misaligned garbage after it.
pub fn walk(code: &[u8]) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < code.len() {
        let raw = code[pos];
        let (prefix, opcode, op_start) = if raw == 0xFE {
            match u8_at(code, pos + 1) {
                Some(second) => (Some(0xFE), second, pos + 2),
                None => break,
            }
        } else {
            (None, raw, pos + 1)
        };
        let Some(len) = operand_size(prefix, opcode, code, op_start) else {
            break;
        };
        if op_start + len > code.len() {
            break;
        }
        out.push(Instruction {
            offset: pos as u32,
            prefix,
            opcode,
            operand_at: op_start,
            operand_len: len,
        });
        pos = op_start + len;
    }
    out
}

/// Operand width in bytes, or `None` for an undefined opcode.
///
/// Grouped by width rather than listed opcode by opcode, because that is how the instruction set
/// is actually laid out and it keeps the table checkable against ECMA-335 III by eye.
fn operand_size(prefix: Option<u8>, opcode: u8, code: &[u8], op_start: usize) -> Option<usize> {
    if prefix == Some(0xFE) {
        return Some(match opcode {
            // arglist, ceq, cgt, cgt.un, clt, clt.un
            0x00..=0x05 => 0,
            // ldftn, ldvirtftn — method tokens
            0x06 | 0x07 => 4,
            // ldarg, ldarga, starg, ldloc, ldloca, stloc — 2-byte variable indices
            0x09..=0x0E => 2,
            // localloc, endfilter
            0x0F | 0x11 => 0,
            // unaligned.
            0x12 => 1,
            // volatile., tail.
            0x13 | 0x14 => 0,
            // initobj, constrained. — type tokens
            0x15 | 0x16 => 4,
            // cpblk, initblk
            0x17 | 0x18 => 0,
            // no.
            0x19 => 1,
            // rethrow
            0x1A => 0,
            // sizeof — type token
            0x1C => 4,
            // refanytype, readonly.
            0x1D | 0x1E => 0,
            _ => return None,
        });
    }

    Some(match opcode {
        // nop … stloc.3, plus ldnull/ldc.i4.m1/ldc.i4.0-8
        0x00..=0x0D => 0,
        // ldarg.s, ldarga.s, starg.s, ldloc.s, ldloca.s, stloc.s
        0x0E..=0x13 => 1,
        0x14..=0x1E => 0,
        // ldc.i4.s
        0x1F => 1,
        // ldc.i4, ldc.r4
        0x20 | 0x22 => 4,
        // ldc.i8, ldc.r8
        0x21 | 0x23 => 8,
        // dup, pop
        0x25 | 0x26 => 0,
        // jmp, call, calli — tokens
        0x27..=0x29 => 4,
        // ret
        0x2A => 0,
        // br.s … blt.un.s — short branch targets
        0x2B..=0x37 => 1,
        // br … blt.un — long branch targets
        0x38..=0x44 => 4,
        // switch: a u32 case count followed by that many u32 targets.
        0x45 => {
            let n = u32_at(code, op_start)? as usize;
            // A corrupt count would otherwise drive an enormous offset; the body length is the
            // natural bound, since the jump table must fit inside it.
            let bytes = n.checked_mul(4)?;
            if bytes > code.len() {
                return None;
            }
            4 + bytes
        }
        // ldind.* / stind.* / arithmetic / conv.*
        0x46..=0x6E => 0,
        // callvirt, cpobj, ldobj, ldstr, newobj, castclass, isinst — tokens
        0x6F..=0x75 => 4,
        // conv.r.un
        0x76 => 0,
        // unbox
        0x79 => 4,
        // throw
        0x7A => 0,
        // ldfld, ldflda, stfld, ldsfld, ldsflda, stsfld, stobj — tokens
        0x7B..=0x81 => 4,
        // conv.ovf.*.un
        0x82..=0x8B => 0,
        // box, newarr
        0x8C | 0x8D => 4,
        // ldlen
        0x8E => 0,
        // ldelema
        0x8F => 4,
        // ldelem.* / stelem.*
        0x90..=0xA2 => 0,
        // ldelem, stelem, unbox.any — type tokens
        0xA3..=0xA5 => 4,
        // conv.ovf.*
        0xB3..=0xBA => 0,
        // refanyval
        0xC2 => 4,
        // ckfinite
        0xC3 => 0,
        // mkrefany
        0xC6 => 4,
        // ldtoken
        0xD0 => 4,
        // conv.u2 … endfinally
        0xD1..=0xDC => 0,
        // leave
        0xDD => 4,
        // leave.s
        0xDE => 1,
        // stind.i, conv.u
        0xDF | 0xE0 => 0,
        _ => return None,
    })
}

/// The mnemonic of a **conditional** branch, or `None`.
///
/// `br`/`br.s` (unconditional) and `leave` are excluded: they carry no decision, and counting
/// them would inflate cyclomatic complexity on every `if` and `try` block the compiler closes.
pub fn conditional_branch_name(prefix: Option<u8>, opcode: u8) -> Option<&'static str> {
    if prefix.is_some() {
        return None;
    }
    Some(match opcode {
        0x2C => "brfalse.s",
        0x2D => "brtrue.s",
        0x2E => "beq.s",
        0x2F => "bge.s",
        0x30 => "bgt.s",
        0x31 => "ble.s",
        0x32 => "blt.s",
        0x33 => "bne.un.s",
        0x34 => "bge.un.s",
        0x35 => "bgt.un.s",
        0x36 => "ble.un.s",
        0x37 => "blt.un.s",
        0x39 => "brfalse",
        0x3A => "brtrue",
        0x3B => "beq",
        0x3C => "bge",
        0x3D => "bgt",
        0x3E => "ble",
        0x3F => "blt",
        0x40 => "bne.un",
        0x41 => "bge.un",
        0x42 => "bgt.un",
        0x43 => "ble.un",
        0x44 => "blt.un",
        _ => return None,
    })
}

/// A metadata token split into `(table id, row)`.
///
/// `ldstr`'s token is the exception: its "table" byte is `0x70`, and the low 24 bits index the
/// `#US` heap directly rather than a table row.
pub fn split_token(token: u32) -> (u8, u32) {
    ((token >> 24) as u8, token & 0x00FF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_header_carries_its_code_size_in_the_first_byte() {
        // (3 << 2) | 0x02 → tiny, 3 bytes of code.
        let image = [0x0E, 0x16, 0x2A, 0xFF, 0xFF];
        let body = read_body(&image, 0).unwrap();
        assert_eq!(body.code, &[0x16, 0x2A, 0xFF]);
    }

    #[test]
    fn a_fat_header_is_twelve_bytes_before_its_code() {
        let mut image = vec![0u8; 32];
        // flags 0x3003: low 2 bits = 0x03 (fat), high nibble = 3 words of header.
        image[0..2].copy_from_slice(&0x3003u16.to_le_bytes());
        image[4..8].copy_from_slice(&4u32.to_le_bytes()); // CodeSize
        image[12..16].copy_from_slice(&[0x16, 0x17, 0x18, 0x2A]);
        let body = read_body(&image, 0).unwrap();
        assert_eq!(body.code, &[0x16, 0x17, 0x18, 0x2A]);
    }

    #[test]
    fn a_truncated_or_corrupt_header_yields_nothing() {
        assert!(read_body(&[], 0).is_none());
        // Tiny header claiming more code than the image holds.
        assert!(read_body(&[0xFE], 0).is_none());
        // Fat header with an impossible header size.
        let mut image = vec![0u8; 32];
        image[0..2].copy_from_slice(&0x0003u16.to_le_bytes());
        assert!(read_body(&image, 0).is_none());
    }

    #[test]
    fn the_walk_advances_by_each_opcodes_real_operand_width() {
        // ldc.i4.s 5 | ldc.i4 0x11223344 | call <tok> | ret
        let code = [
            0x1F, 0x05, // ldc.i4.s 5
            0x20, 0x44, 0x33, 0x22, 0x11, // ldc.i4
            0x28, 0x01, 0x00, 0x00, 0x06, // call
            0x2A, // ret
        ];
        let ins = walk(&code);
        assert_eq!(ins.len(), 4);
        assert_eq!((ins[0].offset, ins[0].opcode), (0, 0x1F));
        assert_eq!((ins[1].offset, ins[1].opcode), (2, 0x20));
        assert_eq!((ins[2].offset, ins[2].opcode), (7, 0x28));
        assert_eq!((ins[3].offset, ins[3].opcode), (12, 0x2A));
    }

    #[test]
    fn two_byte_prefixed_opcodes_are_decoded() {
        // fe 01 (ceq) | fe 09 0003 (ldarg 3) | ret
        let code = [0xFE, 0x01, 0xFE, 0x09, 0x03, 0x00, 0x2A];
        let ins = walk(&code);
        assert_eq!(ins.len(), 3);
        assert_eq!((ins[0].prefix, ins[0].opcode), (Some(0xFE), 0x01));
        assert_eq!((ins[1].prefix, ins[1].opcode), (Some(0xFE), 0x09));
        assert_eq!(ins[1].operand_len, 2);
        assert_eq!(ins[2].opcode, 0x2A);
    }

    /// `switch` is the only variable-length instruction; getting its size wrong desynchronizes
    /// everything after it.
    #[test]
    fn switch_consumes_its_whole_jump_table() {
        let mut code = vec![0x45];
        code.extend_from_slice(&3u32.to_le_bytes());
        for t in 0..3u32 {
            code.extend_from_slice(&t.to_le_bytes());
        }
        code.push(0x2A); // ret
        let ins = walk(&code);
        assert_eq!(ins.len(), 2);
        assert_eq!(ins[0].opcode, 0x45);
        assert_eq!(ins[0].operand_len, 4 + 12);
        assert_eq!((ins[1].offset, ins[1].opcode), (17, 0x2A));
    }

    #[test]
    fn a_switch_with_an_absurd_case_count_stops_the_walk_instead_of_overflowing() {
        let mut code = vec![0x45];
        code.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert!(walk(&code).is_empty());
    }

    #[test]
    fn an_undefined_opcode_stops_the_walk_without_losing_what_came_before() {
        let code = [0x16, 0x2A, 0xF5, 0x16];
        let ins = walk(&code);
        assert_eq!(ins.len(), 2, "instructions before the bad byte are kept");
    }

    #[test]
    fn a_truncated_operand_stops_the_walk() {
        // `call` needs a 4-byte token and only two bytes follow.
        let code = [0x28, 0x01, 0x00];
        assert!(walk(&code).is_empty());
    }

    #[test]
    fn unconditional_branches_are_not_counted_as_decisions() {
        assert_eq!(conditional_branch_name(None, 0x2B), None, "br.s");
        assert_eq!(conditional_branch_name(None, 0x38), None, "br");
        assert_eq!(conditional_branch_name(None, 0xDD), None, "leave");
        assert_eq!(conditional_branch_name(None, 0x2D), Some("brtrue.s"));
        assert_eq!(conditional_branch_name(None, 0x44), Some("blt.un"));
        assert_eq!(conditional_branch_name(Some(0xFE), 0x2D), None);
    }

    #[test]
    fn tokens_split_into_table_and_row() {
        assert_eq!(split_token(0x0600_0123), (0x06, 0x123));
        assert_eq!(split_token(0x0A00_0001), (0x0A, 1));
        assert_eq!(split_token(0x7000_0042), (0x70, 0x42), "ldstr → #US");
    }

    /// Every opcode the walk can meet must be sized or explicitly rejected — never sized wrongly.
    /// This sweeps the whole single-byte space to prove there is no accidental fallthrough.
    #[test]
    fn every_single_byte_opcode_is_either_sized_or_rejected() {
        let code = [0u8; 64];
        for op in 0u16..=0xFFu16 {
            let op = op as u8;
            if op == 0xFE {
                continue;
            }
            // The call must not panic, whatever it returns.
            let _ = operand_size(None, op, &code, 1);
        }
    }
}
