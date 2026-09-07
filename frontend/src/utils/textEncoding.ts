function hasPrefix(bytes: Uint8Array, prefix: number[]) {
  return prefix.every((value, index) => bytes[index] === value);
}

function decode(bytes: Uint8Array, encoding: string, fatal = true) {
  return new TextDecoder(encoding, { fatal }).decode(bytes);
}

export async function decodeTextBlob(blob: Blob) {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  if (hasPrefix(bytes, [0xff, 0xfe])) {
    return decode(bytes, "utf-16le", false);
  }
  if (hasPrefix(bytes, [0xfe, 0xff])) {
    return decode(bytes, "utf-16be", false);
  }

  try {
    return decode(bytes, "utf-8");
  } catch {
    try {
      return decode(bytes, "gb18030");
    } catch {
      return decode(bytes, "utf-8", false);
    }
  }
}
