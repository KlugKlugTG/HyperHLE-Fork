/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! CommonCrypto

use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::Environment;

// CCCryptorStatus
const kCCSuccess: i32 = 0;
const kCCParamError: i32 = -4300;
const kCCBufferTooSmall: i32 = -4301;
const kCCAlignmentError: i32 = -4303;
const kCCDecodeError: i32 = -4304;

// Вспомогательные функции для чтения и записи u32 (Little Endian)
fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}
fn write_u32_le(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
}

// Трансформация блока MD5
fn md5_step(state: &mut [u32; 4], data: &[u8; 64]) {
    let mut words = [0u32; 16];
    for i in 0..16 {
        words[i] = u32::from_le_bytes([
            data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]
        ]);
    }
    let [mut a, mut b, mut c, mut d] = *state;

    let s = [
        7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,
        5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,
        4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,
        6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,
    ];
    let k = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    for i in 0..64 {
        let (mut f, g) = match i {
            0..=15 => ((b & c) | (!b & d), i),
            16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
            48..=63 => (c ^ (b | !d), (7 * i) % 16),
            _ => unreachable!(),
        };
        f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(words[g]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f.rotate_left(s[i]));
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[allow(non_snake_case)]
fn CC_MD5_Init(env: &mut Environment, c: MutVoidPtr) -> i32 {
    if c.is_null() { return 0; }
    let c_ptr = c.cast::<u8>();
    
    // CC_MD5_CTX занимает 92 байта в памяти
    let mut ctx = [0u8; 92];
    write_u32_le(&mut ctx, 0, 0x67452301);
    write_u32_le(&mut ctx, 4, 0xefcdab89);
    write_u32_le(&mut ctx, 8, 0x98badcfe);
    write_u32_le(&mut ctx, 12, 0x10325476);
    
    env.mem.bytes_at_mut(c_ptr, 92).copy_from_slice(&ctx);
    1
}

#[allow(non_snake_case)]
fn CC_MD5_Update(env: &mut Environment, c: MutVoidPtr, data: ConstVoidPtr, len: GuestUSize) -> i32 {
    if c.is_null() || data.is_null() || len == 0 { return 1; }
    let c_ptr = c.cast::<u8>();
    let data_ptr = data.cast::<u8>();

    let mut ctx = env.mem.bytes_at(c_ptr, 92).to_vec();
    let input = env.mem.bytes_at(data_ptr, len).to_vec();

    let mut state = [
        read_u32_le(&ctx, 0), read_u32_le(&ctx, 4),
        read_u32_le(&ctx, 8), read_u32_le(&ctx, 12)
    ];
    let mut nl = read_u32_le(&ctx, 16);
    let mut nh = read_u32_le(&ctx, 20);
    let mut num = read_u32_le(&ctx, 88) as usize;

    let bits = (len as u64) * 8;
    let nl_new = nl as u64 + bits;
    nl = nl_new as u32;
    nh = nh.wrapping_add((nl_new >> 32) as u32);

    let mut input_idx = 0;
    let input_len = len as usize;

    while input_idx < input_len {
        let space = 64 - num;
        let chunk = std::cmp::min(space, input_len - input_idx);
        ctx[24 + num .. 24 + num + chunk].copy_from_slice(&input[input_idx .. input_idx + chunk]);
        num += chunk;
        input_idx += chunk;

        if num == 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&ctx[24..88]);
            md5_step(&mut state, &block);
            num = 0;
        }
    }

    write_u32_le(&mut ctx, 0, state[0]);
    write_u32_le(&mut ctx, 4, state[1]);
    write_u32_le(&mut ctx, 8, state[2]);
    write_u32_le(&mut ctx, 12, state[3]);
    write_u32_le(&mut ctx, 16, nl);
    write_u32_le(&mut ctx, 20, nh);
    write_u32_le(&mut ctx, 88, num as u32);

    env.mem.bytes_at_mut(c_ptr, 92).copy_from_slice(&ctx);
    1
}

#[allow(non_snake_case)]
fn CC_MD5_Final(env: &mut Environment, md: MutVoidPtr, c: MutVoidPtr) -> i32 {
    if md.is_null() || c.is_null() { return 0; }
    let md_ptr = md.cast::<u8>();
    let c_ptr = c.cast::<u8>();

    let mut ctx = env.mem.bytes_at(c_ptr, 92).to_vec();
    let mut state = [
        read_u32_le(&ctx, 0), read_u32_le(&ctx, 4),
        read_u32_le(&ctx, 8), read_u32_le(&ctx, 12)
    ];
    let nl = read_u32_le(&ctx, 16);
    let nh = read_u32_le(&ctx, 20);
    let mut num = read_u32_le(&ctx, 88) as usize;

    ctx[24 + num] = 0x80;
    num += 1;

    if num > 56 {
        for i in num..64 { ctx[24 + i] = 0; }
        let mut block = [0u8; 64];
        block.copy_from_slice(&ctx[24..88]);
        md5_step(&mut state, &block);
        num = 0;
    }

    for i in num..56 { ctx[24 + i] = 0; }

    ctx[24 + 56 .. 24 + 60].copy_from_slice(&nl.to_le_bytes());
    ctx[24 + 60 .. 24 + 64].copy_from_slice(&nh.to_le_bytes());

    let mut block = [0u8; 64];
    block.copy_from_slice(&ctx[24..88]);
    md5_step(&mut state, &block);

    let mut hash = [0u8; 16];
    hash[0..4].copy_from_slice(&state[0].to_le_bytes());
    hash[4..8].copy_from_slice(&state[1].to_le_bytes());
    hash[8..12].copy_from_slice(&state[2].to_le_bytes());
    hash[12..16].copy_from_slice(&state[3].to_le_bytes());

    env.mem.bytes_at_mut(md_ptr, 16).copy_from_slice(&hash);
    env.mem.bytes_at_mut(c_ptr, 92).copy_from_slice(&[0u8; 92]);

    1
}

// AES S-box
const AES_SBOX: [u8; 256] = [
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
    0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
    0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
    0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
    0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
    0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
    0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
    0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
    0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
    0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
    0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
];

// AES inverse S-box
const AES_INV_SBOX: [u8; 256] = [
    0x52,0x09,0x6a,0xd5,0x30,0x36,0xa5,0x38,0xbf,0x40,0xa3,0x9e,0x81,0xf3,0xd7,0xfb,
    0x7c,0xe3,0x39,0x82,0x9b,0x2f,0xff,0x87,0x34,0x8e,0x43,0x44,0xc4,0xde,0xe9,0xcb,
    0x54,0x7b,0x94,0x32,0xa6,0xc2,0x23,0x3d,0xee,0x4c,0x95,0x0b,0x42,0xfa,0xc3,0x4e,
    0x08,0x2e,0xa1,0x66,0x28,0xd9,0x24,0xb2,0x76,0x5b,0xa2,0x49,0x6d,0x8b,0xd1,0x25,
    0x72,0xf8,0xf6,0x64,0x86,0x68,0x98,0x16,0xd4,0xa4,0x5c,0xcc,0x5d,0x65,0xb6,0x92,
    0x6c,0x70,0x48,0x50,0xfd,0xed,0xb9,0xda,0x5e,0x15,0x46,0x57,0xa7,0x8d,0x9d,0x84,
    0x90,0xd8,0xab,0x00,0x8c,0xbc,0xd3,0x0a,0xf7,0xe4,0x58,0x05,0xb8,0xb3,0x45,0x06,
    0xd0,0x2c,0x1e,0x8f,0xca,0x3f,0x0f,0x02,0xc1,0xaf,0xbd,0x03,0x01,0x13,0x8a,0x6b,
    0x3a,0x91,0x11,0x41,0x4f,0x67,0xdc,0xea,0x97,0xf2,0xcf,0xce,0xf0,0xb4,0xe6,0x73,
    0x96,0xac,0x74,0x22,0xe7,0xad,0x35,0x85,0xe2,0xf9,0x37,0xe8,0x1c,0x75,0xdf,0x6e,
    0x47,0xf1,0x1a,0x71,0x1d,0x29,0xc5,0x89,0x6f,0xb7,0x62,0x0e,0xaa,0x18,0xbe,0x1b,
    0xfc,0x56,0x3e,0x4b,0xc6,0xd2,0x79,0x20,0x9a,0xdb,0xc0,0xfe,0x78,0xcd,0x5a,0xf4,
    0x1f,0xdd,0xa8,0x33,0x88,0x07,0xc7,0x31,0xb1,0x12,0x10,0x59,0x27,0x80,0xec,0x5f,
    0x60,0x51,0x7f,0xa9,0x19,0xb5,0x4a,0x0d,0x2d,0xe5,0x7a,0x9f,0x93,0xc9,0x9c,0xef,
    0xa0,0xe0,0x3b,0x4d,0xae,0x2a,0xf5,0xb0,0xc8,0xeb,0xbb,0x3c,0x83,0x53,0x99,0x61,
    0x17,0x2b,0x04,0x7e,0xba,0x77,0xd6,0x26,0xe1,0x69,0x14,0x63,0x55,0x21,0x0c,0x7d,
];

// AES round constants
const AES_RCON: [u8; 10] = [0x01,0x02,0x04,0x08,0x10,0x20,0x40,0x80,0x1b,0x36];

fn aes_key_expansion(key: &[u8], nk: usize, nr: usize) -> Vec<u8> {
    let nb = 4;
    let total_words = nb * (nr + 1);
    let mut w = vec![0u32; total_words];

    for i in 0..nk {
        w[i] = u32::from_be_bytes([key[4*i], key[4*i+1], key[4*i+2], key[4*i+3]]);
    }

    for i in nk..total_words {
        let mut temp = w[i - 1];
        if i % nk == 0 {
            // RotWord + SubWord + Rcon
            temp = temp.rotate_left(8);
            let b = temp.to_be_bytes();
            temp = u32::from_be_bytes([
                AES_SBOX[b[0] as usize],
                AES_SBOX[b[1] as usize],
                AES_SBOX[b[2] as usize],
                AES_SBOX[b[3] as usize],
            ]);
            temp ^= (AES_RCON[i / nk - 1] as u32) << 24;
        } else if nk > 6 && i % nk == 4 {
            let b = temp.to_be_bytes();
            temp = u32::from_be_bytes([
                AES_SBOX[b[0] as usize],
                AES_SBOX[b[1] as usize],
                AES_SBOX[b[2] as usize],
                AES_SBOX[b[3] as usize],
            ]);
        }
        w[i] = w[i - nk] ^ temp;
    }

    let mut expanded = vec![0u8; total_words * 4];
    for (i, &word) in w.iter().enumerate() {
        expanded[4*i..4*i+4].copy_from_slice(&word.to_be_bytes());
    }
    expanded
}

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut result: u8 = 0;
    for _ in 0..8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    result
}

fn aes_encrypt_block(block: &[u8; 16], expanded_key: &[u8], nr: usize) -> [u8; 16] {
    let mut state = *block;

    // AddRoundKey (round 0)
    for i in 0..16 {
        state[i] ^= expanded_key[i];
    }

    for round in 1..nr {
        let rk_off = round * 16;

        // SubBytes
        for b in &mut state {
            *b = AES_SBOX[*b as usize];
        }

        // ShiftRows (state is column-major: index = row + 4*col)
        let tmp = state[1];
        state[1] = state[5]; state[5] = state[9]; state[9] = state[13]; state[13] = tmp;
        let tmp0 = state[2]; let tmp1 = state[6];
        state[2] = state[10]; state[6] = state[14]; state[10] = tmp0; state[14] = tmp1;
        let tmp = state[15];
        state[15] = state[11]; state[11] = state[7]; state[7] = state[3]; state[3] = tmp;

        // MixColumns
        for c in 0..4 {
            let i = c * 4;
            let s0 = state[i]; let s1 = state[i+1]; let s2 = state[i+2]; let s3 = state[i+3];
            state[i]   = gf_mul(2, s0) ^ gf_mul(3, s1) ^ s2 ^ s3;
            state[i+1] = s0 ^ gf_mul(2, s1) ^ gf_mul(3, s2) ^ s3;
            state[i+2] = s0 ^ s1 ^ gf_mul(2, s2) ^ gf_mul(3, s3);
            state[i+3] = gf_mul(3, s0) ^ s1 ^ s2 ^ gf_mul(2, s3);
        }

        // AddRoundKey
        for i in 0..16 {
            state[i] ^= expanded_key[rk_off + i];
        }
    }

    // Final round (no MixColumns)
    for b in &mut state {
        *b = AES_SBOX[*b as usize];
    }

    let tmp = state[1];
    state[1] = state[5]; state[5] = state[9]; state[9] = state[13]; state[13] = tmp;
    let tmp0 = state[2]; let tmp1 = state[6];
    state[2] = state[10]; state[6] = state[14]; state[10] = tmp0; state[14] = tmp1;
    let tmp = state[15];
    state[15] = state[11]; state[11] = state[7]; state[7] = state[3]; state[3] = tmp;

    let rk_off = nr * 16;
    for i in 0..16 {
        state[i] ^= expanded_key[rk_off + i];
    }

    state
}

fn aes_decrypt_block(block: &[u8; 16], expanded_key: &[u8], nr: usize) -> [u8; 16] {
    let mut state = *block;

    // AddRoundKey (last round key)
    let rk_off = nr * 16;
    for i in 0..16 {
        state[i] ^= expanded_key[rk_off + i];
    }

    for round in (1..nr).rev() {
        let rk_off = round * 16;

        // InvShiftRows
        let tmp = state[13];
        state[13] = state[9]; state[9] = state[5]; state[5] = state[1]; state[1] = tmp;
        let tmp0 = state[10]; let tmp1 = state[14];
        state[10] = state[2]; state[14] = state[6]; state[2] = tmp0; state[6] = tmp1;
        let tmp = state[3];
        state[3] = state[7]; state[7] = state[11]; state[11] = state[15]; state[15] = tmp;

        // InvSubBytes
        for b in &mut state {
            *b = AES_INV_SBOX[*b as usize];
        }

        // AddRoundKey
        for i in 0..16 {
            state[i] ^= expanded_key[rk_off + i];
        }

        // InvMixColumns
        for c in 0..4 {
            let i = c * 4;
            let s0 = state[i]; let s1 = state[i+1]; let s2 = state[i+2]; let s3 = state[i+3];
            state[i]   = gf_mul(0x0e, s0) ^ gf_mul(0x0b, s1) ^ gf_mul(0x0d, s2) ^ gf_mul(0x09, s3);
            state[i+1] = gf_mul(0x09, s0) ^ gf_mul(0x0e, s1) ^ gf_mul(0x0b, s2) ^ gf_mul(0x0d, s3);
            state[i+2] = gf_mul(0x0d, s0) ^ gf_mul(0x09, s1) ^ gf_mul(0x0e, s2) ^ gf_mul(0x0b, s3);
            state[i+3] = gf_mul(0x0b, s0) ^ gf_mul(0x0d, s1) ^ gf_mul(0x09, s2) ^ gf_mul(0x0e, s3);
        }
    }

    // Final inverse round (no InvMixColumns)
    let tmp = state[13];
    state[13] = state[9]; state[9] = state[5]; state[5] = state[1]; state[1] = tmp;
    let tmp0 = state[10]; let tmp1 = state[14];
    state[10] = state[2]; state[14] = state[6]; state[2] = tmp0; state[6] = tmp1;
    let tmp = state[3];
    state[3] = state[7]; state[7] = state[11]; state[11] = state[15]; state[15] = tmp;

    for b in &mut state {
        *b = AES_INV_SBOX[*b as usize];
    }

    for i in 0..16 {
        state[i] ^= expanded_key[i];
    }

    state
}

// CCCrypt has 11 args. All are passed via the standard ARM calling convention
// (R0-R3 + stack), handled by the CallFromGuest framework.
#[allow(non_snake_case)]
fn CCCrypt(
    env: &mut Environment,
    op: u32,
    alg: u32,
    options: u32,
    key: ConstVoidPtr,
    key_length: GuestUSize,
    iv: ConstVoidPtr,
    data_in: ConstVoidPtr,
    data_in_length: GuestUSize,
    data_out: MutVoidPtr,
    data_out_available: GuestUSize,
    data_out_moved: MutPtr<GuestUSize>,
) -> i32 {
    log!(
        "CCCrypt(op={}, alg={}, options={:#x}, keyLen={}, dataLen={})",
        op, alg, options, key_length, data_in_length
    );

    let ecb_mode = (options & 0x2) != 0;
    let pkcs7_pad = (options & 0x1) != 0;

    // RC4 stream cipher (alg == 4)
    if alg == 4 {
        if data_out_available < data_in_length {
            return kCCBufferTooSmall;
        }
        let input = env.mem.bytes_at(data_in.cast(), data_in_length).to_vec();
        let key_bytes = env.mem.bytes_at(key.cast(), key_length).to_vec();
        let mut output = vec![0u8; data_in_length as usize];

        let mut s: Vec<u8> = (0..=255u8).collect();
        let mut j: usize = 0;
        for i in 0..256usize {
            j = (j + s[i] as usize + key_bytes[i % key_length as usize] as usize) % 256;
            s.swap(i, j);
        }
        let mut i = 0usize;
        j = 0;
        for (idx, &byte) in input.iter().enumerate() {
            i = (i + 1) % 256;
            j = (j + s[i] as usize) % 256;
            s.swap(i, j);
            let k = s[(s[i] as usize + s[j] as usize) % 256];
            output[idx] = byte ^ k;
        }
        env.mem.bytes_at_mut(data_out.cast(), data_in_length).copy_from_slice(&output);
        env.mem.write(data_out_moved, data_in_length);
        return kCCSuccess;
    }

    // Determine block size and number of rounds based on algorithm
    let block_size: usize = match alg {
        0 => 16, // kCCAlgorithmAES128
        1 => 8,  // kCCAlgorithmDES
        2 => 8,  // kCCAlgorithm3DES
        3 => 8,  // kCCAlgorithmCAST
        _ => {
            log!("CCCrypt: alg={} not supported, data copied as-is", alg);
            if data_out_available < data_in_length {
                return kCCBufferTooSmall;
            }
            let input = env.mem.bytes_at(data_in.cast(), data_in_length).to_vec();
            env.mem.bytes_at_mut(data_out.cast(), data_in_length).copy_from_slice(&input);
            env.mem.write(data_out_moved, data_in_length);
            return kCCSuccess;
        }
    };

    // AES block cipher
    if alg == 0 {
        let (nk, nr) = match key_length {
            16 => (4, 10), // AES-128
            24 => (6, 12), // AES-192
            32 => (8, 14), // AES-256
            _ => {
                log!("CCCrypt: unsupported AES key length {}", key_length);
                return kCCParamError;
            }
        };

        let key_bytes = env.mem.bytes_at(key.cast(), key_length).to_vec();
        let expanded_key = aes_key_expansion(&key_bytes, nk, nr);

        let input = env.mem.bytes_at(data_in.cast(), data_in_length).to_vec();
        let input_len = data_in_length as usize;
        let is_encrypt = op == 0;

        let mut output: Vec<u8>;

        if is_encrypt {
            let padded: Vec<u8>;
            let work_data = if pkcs7_pad {
                let pad_len = block_size - (input_len % block_size);
                padded = input.iter().copied()
                    .chain(std::iter::repeat(pad_len as u8).take(pad_len))
                    .collect();
                &padded
            } else {
                if input_len % block_size != 0 {
                    return kCCAlignmentError;
                }
                &input
            };

            let out_len = work_data.len();
            if data_out_available < out_len as GuestUSize {
                return kCCBufferTooSmall;
            }

            output = vec![0u8; out_len];
            let mut prev_block = [0u8; 16];
            if !ecb_mode && !iv.is_null() {
                prev_block.copy_from_slice(&env.mem.bytes_at(iv.cast(), 16).to_vec());
            }

            for i in (0..out_len).step_by(block_size) {
                let mut blk = [0u8; 16];
                blk.copy_from_slice(&work_data[i..i + block_size]);

                if !ecb_mode {
                    for j in 0..block_size {
                        blk[j] ^= prev_block[j];
                    }
                }

                let encrypted = aes_encrypt_block(&blk, &expanded_key, nr);
                output[i..i + block_size].copy_from_slice(&encrypted);

                if !ecb_mode {
                    prev_block.copy_from_slice(&encrypted);
                }
            }

            env.mem.bytes_at_mut(data_out.cast(), out_len as GuestUSize).copy_from_slice(&output);
            env.mem.write(data_out_moved, out_len as GuestUSize);
        } else {
            // Decrypt
            if input_len == 0 {
                env.mem.write(data_out_moved, 0);
                return kCCSuccess;
            }
            if input_len % block_size != 0 {
                return kCCAlignmentError;
            }

            output = vec![0u8; input_len];
            let mut prev_block = [0u8; 16];
            if !ecb_mode && !iv.is_null() {
                prev_block.copy_from_slice(&env.mem.bytes_at(iv.cast(), 16).to_vec());
            }

            for i in (0..input_len).step_by(block_size) {
                let mut blk = [0u8; 16];
                blk.copy_from_slice(&input[i..i + block_size]);

                let decrypted = aes_decrypt_block(&blk, &expanded_key, nr);

                if ecb_mode {
                    output[i..i + block_size].copy_from_slice(&decrypted);
                } else {
                    for j in 0..block_size {
                        output[i + j] = decrypted[j] ^ prev_block[j];
                    }
                    prev_block.copy_from_slice(&input[i..i + block_size]);
                }
            }

            let out_len = if pkcs7_pad {
                let pad = output[input_len - 1] as usize;
                if pad == 0 || pad > block_size {
                    return kCCDecodeError;
                }
                input_len - pad
            } else {
                input_len
            };

            if data_out_available < out_len as GuestUSize {
                return kCCBufferTooSmall;
            }

            env.mem.bytes_at_mut(data_out.cast(), out_len as GuestUSize).copy_from_slice(&output[..out_len]);
            env.mem.write(data_out_moved, out_len as GuestUSize);
        }

        return kCCSuccess;
    }

    // Unsupported block cipher algorithm: copy as-is
    if data_out_available < data_in_length {
        return kCCBufferTooSmall;
    }
    let input = env.mem.bytes_at(data_in.cast(), data_in_length).to_vec();
    env.mem.bytes_at_mut(data_out.cast(), data_in_length).copy_from_slice(&input);
    env.mem.write(data_out_moved, data_in_length);
    log!("CCCrypt: alg={} not implemented, data copied as-is", alg);
    kCCSuccess
}

#[allow(non_snake_case)]
fn CCKeyDerivationPBKDF(
    _env: &mut Environment,
    _algorithm: u32,
    _password: ConstVoidPtr,
    _password_len: GuestUSize,
    _salt: ConstVoidPtr,
    _salt_len: GuestUSize,
    _prf: u32,
    _rounds: u32,
) -> i32 {
    log!("TODO: CCKeyDerivationPBKDF");
    kCCSuccess
}

// One-shot MD5 hash (host-side, no guest memory)
fn md5_hash(data: &[u8]) -> [u8; 16] {
    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let bit_len = (data.len() as u64) * 8;

    // Process complete 64-byte blocks
    let mut offset = 0;
    while offset + 64 <= data.len() {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[offset..offset + 64]);
        md5_step(&mut state, &block);
        offset += 64;
    }

    // Padding
    let mut last = Vec::with_capacity(128);
    last.extend_from_slice(&data[offset..]);
    last.push(0x80);
    while last.len() % 64 != 56 {
        last.push(0);
    }
    last.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in last.chunks_exact(64) {
        let mut block = [0u8; 64];
        block.copy_from_slice(chunk);
        md5_step(&mut state, &block);
    }

    let mut hash = [0u8; 16];
    hash[0..4].copy_from_slice(&state[0].to_le_bytes());
    hash[4..8].copy_from_slice(&state[1].to_le_bytes());
    hash[8..12].copy_from_slice(&state[2].to_le_bytes());
    hash[12..16].copy_from_slice(&state[3].to_le_bytes());
    hash
}

// SHA-1 block transform
fn sha1_step(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for i in 0..80 {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
            60..=79 => (b ^ c ^ d, 0xCA62C1D6u32),
            _ => unreachable!(),
        };
        let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
        e = d; d = c; c = b.rotate_left(30); b = a; a = temp;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

// One-shot SHA-1 hash
fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;

    let mut offset = 0;
    while offset + 64 <= data.len() {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[offset..offset + 64]);
        sha1_step(&mut state, &block);
        offset += 64;
    }

    let mut last = Vec::with_capacity(128);
    last.extend_from_slice(&data[offset..]);
    last.push(0x80);
    while last.len() % 64 != 56 {
        last.push(0);
    }
    last.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in last.chunks_exact(64) {
        let mut block = [0u8; 64];
        block.copy_from_slice(chunk);
        sha1_step(&mut state, &block);
    }

    let mut hash = [0u8; 20];
    hash[0..4].copy_from_slice(&state[0].to_be_bytes());
    hash[4..8].copy_from_slice(&state[1].to_be_bytes());
    hash[8..12].copy_from_slice(&state[2].to_be_bytes());
    hash[12..16].copy_from_slice(&state[3].to_be_bytes());
    hash[16..20].copy_from_slice(&state[4].to_be_bytes());
    hash
}

// SHA-256 block transform
fn sha256_step(state: &mut [u32; 8], block: &[u8; 64]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g; g = f; f = e; e = d.wrapping_add(temp1);
        d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

// One-shot SHA-256 hash
fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;

    let mut offset = 0;
    while offset + 64 <= data.len() {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[offset..offset + 64]);
        sha256_step(&mut state, &block);
        offset += 64;
    }

    let mut last = Vec::with_capacity(128);
    last.extend_from_slice(&data[offset..]);
    last.push(0x80);
    while last.len() % 64 != 56 {
        last.push(0);
    }
    last.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in last.chunks_exact(64) {
        let mut block = [0u8; 64];
        block.copy_from_slice(chunk);
        sha256_step(&mut state, &block);
    }

    let mut hash = [0u8; 32];
    for i in 0..8 {
        hash[i * 4..(i + 1) * 4].copy_from_slice(&state[i].to_be_bytes());
    }
    hash
}

#[allow(non_snake_case)]
fn CCHmac(
    env: &mut Environment,
    algorithm: u32,
    key: ConstVoidPtr,
    key_length: GuestUSize,
    data: ConstVoidPtr,
    data_length: GuestUSize,
    mac_out: MutVoidPtr,
) {
    // algorithm: 0=SHA1, 1=MD5, 2=SHA256, 3=SHA384, 4=SHA512, 5=SHA224
    let (block_size, hash_len): (usize, usize) = match algorithm {
        0 => (64, 20),  // kCCHmacAlgSHA1
        1 => (64, 16),  // kCCHmacAlgMD5
        2 => (64, 32),  // kCCHmacAlgSHA256
        _ => {
            log!("CCHmac: unsupported algorithm {}, writing zeros", algorithm);
            return;
        }
    };

    log!(
        "CCHmac(algorithm={}, keyLen={}, dataLen={})",
        algorithm, key_length, data_length
    );

    let key_bytes = env.mem.bytes_at(key.cast(), key_length).to_vec();
    let data_bytes = env.mem.bytes_at(data.cast(), data_length).to_vec();

    // If key is longer than block size, hash it first
    let key_block = if key_bytes.len() > block_size {
        match algorithm {
            0 => sha1_hash(&key_bytes).to_vec(),
            1 => md5_hash(&key_bytes).to_vec(),
            2 => sha256_hash(&key_bytes).to_vec(),
            _ => unreachable!(),
        }
    } else {
        key_bytes
    };

    // Pad key to block_size
    let mut k_ipad = vec![0x36u8; block_size];
    let mut k_opad = vec![0x5cu8; block_size];
    for i in 0..key_block.len() {
        k_ipad[i] ^= key_block[i];
        k_opad[i] ^= key_block[i];
    }

    // inner = H(k_ipad || data)
    let mut inner_data = k_ipad;
    inner_data.extend_from_slice(&data_bytes);

    let inner_hash: Vec<u8> = match algorithm {
        0 => sha1_hash(&inner_data).to_vec(),
        1 => md5_hash(&inner_data).to_vec(),
        2 => sha256_hash(&inner_data).to_vec(),
        _ => unreachable!(),
    };

    // outer = H(k_opad || inner_hash)
    let mut outer_data = k_opad;
    outer_data.extend_from_slice(&inner_hash);

    let result: Vec<u8> = match algorithm {
        0 => sha1_hash(&outer_data).to_vec(),
        1 => md5_hash(&outer_data).to_vec(),
        2 => sha256_hash(&outer_data).to_vec(),
        _ => unreachable!(),
    };

    env.mem.bytes_at_mut(mac_out.cast(), hash_len as GuestUSize)
        .copy_from_slice(&result[..hash_len]);
}


// =========================================================================
// MARK: - Security framework stubs (Keychain Services)
// =========================================================================
// These are no-ops — HyperHLE has no keychain. Apps that use keychain
// for license checks or settings will gracefully handle errSecItemNotFound.

// OSStatus error codes
const errSecSuccess:       i32 = 0;
const errSecItemNotFound:  i32 = -25300;
const errSecParam:         i32 = -50;

#[allow(non_snake_case)]
fn SecItemCopyMatching(
    _env: &mut Environment,
    _query: crate::mem::ConstVoidPtr,
    _result: crate::mem::MutVoidPtr,
) -> i32 {
    log_dbg!("SecItemCopyMatching -> errSecItemNotFound (stubbed)");
    errSecItemNotFound
}

#[allow(non_snake_case)]
fn SecItemAdd(
    _env: &mut Environment,
    _attributes: crate::mem::ConstVoidPtr,
    _result: crate::mem::MutVoidPtr,
) -> i32 {
    log_dbg!("SecItemAdd -> errSecSuccess (stubbed)");
    errSecSuccess
}

#[allow(non_snake_case)]
fn SecItemUpdate(
    _env: &mut Environment,
    _query: crate::mem::ConstVoidPtr,
    _attributes_to_update: crate::mem::ConstVoidPtr,
) -> i32 {
    log_dbg!("SecItemUpdate -> errSecItemNotFound (stubbed)");
    errSecItemNotFound
}

#[allow(non_snake_case)]
fn SecItemDelete(
    _env: &mut Environment,
    _query: crate::mem::ConstVoidPtr,
) -> i32 {
    log_dbg!("SecItemDelete -> errSecItemNotFound (stubbed)");
    errSecItemNotFound
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CCCrypt(_, _, _, _, _, _, _, _, _, _, _)),
    export_c_func!(CCKeyDerivationPBKDF(_, _, _, _, _, _, _)), 
    export_c_func!(CCHmac(_, _, _, _, _, _)),
    // Исправленное количество аргументов (исключая env):
    export_c_func!(CC_MD5_Init(_)),           // Было (_, _), нужно (_)
    export_c_func!(CC_MD5_Update(_, _, _)),    // Было (_, _, _, _), нужно (_, _, _)
    export_c_func!(CC_MD5_Final(_, _)),       // Было (_, _, _), нужно (_, _)
    export_c_func!(SecItemCopyMatching(_, _)),
    export_c_func!(SecItemAdd(_, _)),
    export_c_func!(SecItemUpdate(_, _)),
    export_c_func!(SecItemDelete(_)),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/usr/lib/libcommonCrypto.dylib",
    aliases: &[
        "/System/Library/Frameworks/Security.framework/Security",
        "/usr/lib/libCommonCrypto.dylib",
    ],
    class_exports: &[],
    constant_exports: &[],
    function_exports: &[FUNCTIONS],
};
