export interface Size {
  width: number;
  height: number;
}

/**
 * The displayed size of an image in a box: as large as fits inside `box` minus `padding`
 * on each side, keeping the aspect ratio, but never enlarged beyond 100%.
 */
export function fitContain(image: Size, box: Size, padding = 0): Size & { scale: number } {
  const room = { width: Math.max(0, box.width - 2 * padding), height: Math.max(0, box.height - 2 * padding) };
  if (image.width <= 0 || image.height <= 0) return { width: 0, height: 0, scale: 0 };
  const scale = Math.min(1, room.width / image.width, room.height / image.height);
  return { width: Math.round(image.width * scale), height: Math.round(image.height * scale), scale };
}
