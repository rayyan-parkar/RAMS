/**
 * H.264 Bitstream Utilities for WebCodecs
 * Handles Annex-B (start-code prefixed) and AVCC (length-prefixed) conversions.
 */

export const NAL_TYPE_NAMES: Record<number, string> = {
  1: 'NON_IDR_SLICE', 5: 'IDR_SLICE', 6: 'SEI',
  7: 'SPS', 8: 'PPS', 9: 'AUD', 0: 'UNSPECIFIED'
};

/** Return the NAL unit type (5 LSBs of first byte after start code) */
export function nalType(b: number): number { return b & 0x1f; }

export function nalName(t: number): string { return NAL_TYPE_NAMES[t] ?? `UNKNOWN(${t})`; }

/** Parse SPS and PPS from an AVCDecoderConfigurationRecord (the 'description' field). */
export function parseSPSPPS(desc: ArrayBuffer): { sps: Uint8Array[], pps: Uint8Array[] } {
  const d = new Uint8Array(desc);
  const sps: Uint8Array[] = [];
  const pps: Uint8Array[] = [];
  if (d.length < 7) { return { sps, pps }; }
  // AVCDecoderConfigurationRecord layout:
  // byte 0: configurationVersion (always 1)
  // byte 5 lower 5 bits: numSPS
  let offset = 5;
  const numSPS = d[offset] & 0x1f; offset++;
  for (let i = 0; i < numSPS; i++) {
    const len = (d[offset] << 8) | d[offset + 1]; offset += 2;
    sps.push(d.slice(offset, offset + len)); offset += len;
  }
  const numPPS = d[offset]; offset++;
  for (let i = 0; i < numPPS; i++) {
    const len = (d[offset] << 8) | d[offset + 1]; offset += 2;
    pps.push(d.slice(offset, offset + len)); offset += len;
  }

  return { sps, pps };
}

/** Convert AVCC (length-prefixed NALUs) to Annex-B (start-code-prefixed).
 *  Optionally prepend SPS/PPS before the first NALU. */
export function avccToAnnexB(avcc: Uint8Array, prependNals?: Uint8Array[]): Uint8Array {
  const startCode = new Uint8Array([0, 0, 0, 1]);
  const parts: Uint8Array[] = [];

  // Prepend SPS/PPS if provided
  if (prependNals) {
    for (const nal of prependNals) {
      parts.push(startCode);
      parts.push(nal);
    }
  }

  // Walk the AVCC buffer: [4-byte length][NALU data] ...
  let pos = 0;
  let naluCount = 0;
  while (pos + 4 <= avcc.length) {
    const naluLen = (avcc[pos] << 24) | (avcc[pos+1] << 16) | (avcc[pos+2] << 8) | avcc[pos+3];
    pos += 4;
    if (naluLen <= 0 || pos + naluLen > avcc.length) {
      break;
    }
    naluCount++;
    parts.push(startCode);
    parts.push(avcc.slice(pos, pos + naluLen));
    pos += naluLen;
  }

  // Concatenate all parts
  const totalLen = parts.reduce((sum, p) => sum + p.length, 0);
  const result = new Uint8Array(totalLen);
  let offset = 0;
  for (const p of parts) { result.set(p, offset); offset += p.length; }
  return result;
}

/** Detect if a buffer starts with Annex-B start codes */
export function isAnnexB(data: Uint8Array): boolean {
  return (data.length >= 4 && data[0] === 0 && data[1] === 0 && data[2] === 0 && data[3] === 1) ||
         (data.length >= 3 && data[0] === 0 && data[1] === 0 && data[2] === 1);
}

/** Split Annex-B stream into individual NAL units (without start codes) */
export function parseAnnexB(data: Uint8Array): Uint8Array[] {
  const nals: Uint8Array[] = [];
  let i = 0;
  while (i < data.length) {
    let start = -1;
    for (let j = i; j < data.length - 2; j++) {
      if (data[j] === 0 && data[j+1] === 0 && data[j+2] === 1) {
        const is4Byte = (j > 0 && data[j-1] === 0);
        const scStart = is4Byte ? j - 1 : j;
        const scLen = is4Byte ? 4 : 3;
        if (i < scStart) nals.push(data.subarray(i, scStart));
        start = scStart + scLen;
        break;
      }
    }
    if (start === -1) {
      if (i < data.length) nals.push(data.subarray(i));
      break;
    }
    i = start;
  }
  return nals;
}

/** Convert Annex-B to AVCC (length-prefixed NALUs) */
export function annexBToAVCC(data: Uint8Array): Uint8Array {
  const nals = parseAnnexB(data);
  const totalLen = nals.reduce((sum, n) => sum + 4 + n.length, 0);
  const avcc = new Uint8Array(totalLen);
  let offset = 0;
  for (const n of nals) {
    avcc[offset]   = (n.length >> 24) & 0xff;
    avcc[offset+1] = (n.length >> 16) & 0xff;
    avcc[offset+2] = (n.length >> 8) & 0xff;
    avcc[offset+3] = n.length & 0xff;
    avcc.set(n, offset + 4);
    offset += 4 + n.length;
  }
  return avcc;
}

/** Extract SPS/PPS from Annex-B stream and construct AVCDecoderConfigurationRecord */
export function createAVCCDescriptionFromAnnexB(annexB: Uint8Array): ArrayBuffer | null {
  const nals = parseAnnexB(annexB);
  let sps: Uint8Array | null = null;
  let pps: Uint8Array | null = null;
  for (const nal of nals) {
    const t = nalType(nal[0]);
    if (t === 7 && !sps) sps = nal;
    if (t === 8 && !pps) pps = nal;
  }
  if (!sps || !pps) return null;

  const desc = new Uint8Array(5 + 3 + sps.length + 3 + pps.length);
  desc[0] = 1; // configurationVersion
  desc[1] = sps[1]; // AVCProfileIndication
  desc[2] = sps[2]; // profile_compatibility
  desc[3] = sps[3]; // AVCLevelIndication
  desc[4] = 0xff; // lengthSizeMinusOne = 3 (so 4 bytes)
  desc[5] = 0xe1; // numOfSequenceParameterSets = 1
  desc[6] = (sps.length >> 8) & 0xff;
  desc[7] = sps.length & 0xff;
  desc.set(sps, 8);
  
  const offset = 8 + sps.length;
  desc[offset] = 1; // numOfPictureParameterSets = 1
  desc[offset+1] = (pps.length >> 8) & 0xff;
  desc[offset+2] = pps.length & 0xff;
  desc.set(pps, offset + 3);
  
  return desc.buffer;
}

/** List NAL unit types in an Annex-B stream */
export function listAnnexBNalTypes(data: Uint8Array): string[] {
  const types: string[] = [];
  for (let i = 0; i < data.length - 4; i++) {
    if (data[i] === 0 && data[i+1] === 0 && data[i+2] === 0 && data[i+3] === 1) {
      types.push(nalName(nalType(data[i+4])));
    }
  }
  return types;
}

/** Detect if an Annex-B stream contains an IDR (keyframe) */
export function containsIDR(data: Uint8Array): boolean {
  for (let i = 0; i < data.length - 4; i++) {
    if (data[i] === 0 && data[i+1] === 0 && data[i+2] === 0 && data[i+3] === 1) {
      if (nalType(data[i+4]) === 5) return true;
    }
  }
  return false;
}
