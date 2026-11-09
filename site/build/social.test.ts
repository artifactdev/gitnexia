import { readFileSync } from 'node:fs';
import { expect, test } from 'vitest';
import { OG_IMAGE, siteUrl, socialMarker, socialTags } from './social.ts';

test("the site's address: artifactdev.github.io/gitnexia/ unless the origin or base say otherwise", () => {
  expect(siteUrl()).toBe('https://artifactdev.github.io/gitnexia/');
  expect(siteUrl('https://artifactdev.github.io/', '/gitnexia/')).toBe('https://artifactdev.github.io/gitnexia/');
  expect(() => siteUrl('artifactdev.github.io')).toThrow(/GITNEXIA_SITE_ORIGIN/);
  expect(() => siteUrl('http://artifactdev.github.io')).toThrow(/GITNEXIA_SITE_ORIGIN/);
  expect(() => siteUrl('https://artifactdev.github.io/path')).toThrow(/GITNEXIA_SITE_ORIGIN/);
  expect(() => siteUrl(undefined, '/gitnexia')).toThrow(/GITNEXIA_SITE_BASE/);
});

test('the marker writes absolute URLs for the page and its image, with the image size and alt text', () => {
  const html = socialMarker(undefined, '')('meta');
  expect(html).toContain('<meta property="og:url" content="https://artifactdev.github.io/gitnexia/" />');
  expect(html).toContain('<meta property="og:image" content="https://artifactdev.github.io/gitnexia/og.jpg" />');
  expect(html).toContain('<meta name="twitter:image" content="https://artifactdev.github.io/gitnexia/og.jpg" />');
  expect(html).toContain('<meta property="og:image:width" content="1200" />');
  expect(html).toContain('<meta property="og:image:height" content="630" />');
  expect(socialTags('https://artifactdev.github.io/gitnexia/')).toContain(`<meta property="og:image:alt" content="The GitNexia icon and name over the site's lane lines, the line &quot;A fast,`);
  expect(() => socialMarker()('other')).toThrow(/social:other/);
});

/** A JPEG's size, from its first start-of-frame segment. */
function jpegSize(b: Buffer): { width: number; height: number } {
  for (let i = 2; i < b.length;) {
    const marker = b[i + 1];
    if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) return { height: b.readUInt16BE(i + 5), width: b.readUInt16BE(i + 7) };
    i += 2 + b.readUInt16BE(i + 2);
  }
  throw new Error('no start of frame');
}

test('the committed image (just site-og) is a JPEG of the declared size, small enough for every scraper', () => {
  const jpg = readFileSync(new URL(`../public/${OG_IMAGE.file}`, import.meta.url));
  expect(jpg.subarray(0, 2)).toEqual(Buffer.from([0xff, 0xd8]));
  expect(jpegSize(jpg)).toEqual({ width: OG_IMAGE.width, height: OG_IMAGE.height });
  expect(jpg.length).toBeLessThan(300_000);
});
