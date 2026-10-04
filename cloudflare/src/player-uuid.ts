/** Java UUID.nameUUIDFromBytes("OfflinePlayer:" + canonical, provider-proven name). */
export function offlinePlayerUuid(name: string): string {
  const input = new TextEncoder().encode(`OfflinePlayer:${name}`);
  const bytes = new Uint8Array(Math.ceil((input.length + 9) / 64) * 64);
  bytes.set(input); bytes[input.length] = 0x80;
  const view = new DataView(bytes.buffer);
  view.setUint32(bytes.length - 8, input.length * 8, true);
  const shifts = [7,12,17,22, 5,9,14,20, 4,11,16,23, 6,10,15,21];
  let a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
  for (let offset = 0; offset < bytes.length; offset += 64) {
    let a = a0, b = b0, c = c0, d = d0;
    for (let i = 0; i < 64; i++) {
      const round = Math.floor(i / 16);
      const f = round === 0 ? (b & c) | (~b & d) : round === 1 ? (d & b) | (~d & c) : round === 2 ? b ^ c ^ d : c ^ (b | ~d);
      const g = round === 0 ? i : round === 1 ? (5*i+1)%16 : round === 2 ? (3*i+5)%16 : (7*i)%16;
      const sum = (a + f + Math.floor(Math.abs(Math.sin(i+1)) * 0x100000000) + view.getUint32(offset+g*4, true)) | 0;
      const shift = shifts[round*4 + i%4];
      [a,b,c,d] = [d, (b + ((sum << shift) | (sum >>> (32-shift)))) | 0, b,c];
    }
    a0 = (a0+a)|0; b0 = (b0+b)|0; c0 = (c0+c)|0; d0 = (d0+d)|0;
  }
  const output = new Uint8Array(16); const result = new DataView(output.buffer);
  [a0,b0,c0,d0].forEach((v,i) => result.setUint32(i*4,v,true));
  output[6] = (output[6] & 15) | 0x30; output[8] = (output[8] & 63) | 0x80;
  const hex = Array.from(output, v => v.toString(16).padStart(2,"0")).join("");
  return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
}
