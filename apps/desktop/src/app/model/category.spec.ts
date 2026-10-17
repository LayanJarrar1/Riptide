import { CATEGORY_EXTENSIONS, CATEGORY_KEYS, categoryOf } from './download';

/**
 * A literal copy of the extension table. If this spec fails you changed `CATEGORY_EXTENSIONS`:
 * change `Category::extensions()` in `crates/riptide-core/src/category.rs` (and its pinned
 * test) in the same commit, then this literal. The engine and the UI must classify alike.
 */
const PINNED: Readonly<Record<string, string>> = {
  compressed: 'zip,rar,7z,tar,gz,tgz,bz2,xz,zst,iso,cab',
  documents: 'pdf,doc,docx,xls,xlsx,ppt,pptx,odt,ods,odp,txt,rtf,epub,mobi,csv',
  music: 'mp3,flac,wav,aac,m4a,ogg,opus,wma,aiff',
  video: 'mp4,mkv,avi,mov,wmv,webm,m4v,ts,flv,mpg,mpeg',
  programs: 'exe,msi,msix,appx,apk,dmg,pkg,deb,rpm,appimage',
  images: 'jpg,jpeg,png,gif,webp,bmp,svg,tif,tiff,heic,avif,psd',
  other: '',
};

describe('categories (shared table with riptide-core)', () => {
  it('pins the extension table and its order', () => {
    expect(CATEGORY_KEYS).toEqual(['compressed', 'documents', 'music', 'video', 'programs', 'images', 'other']);
    for (const key of CATEGORY_KEYS) {
      expect(CATEGORY_EXTENSIONS[key].join(',')).withContext(key).toBe(PINNED[key]);
    }
  });

  it('lists every extension once, lower-case and without a dot', () => {
    const seen = new Set<string>();
    for (const key of CATEGORY_KEYS) {
      for (const ext of CATEGORY_EXTENSIONS[key]) {
        expect(ext).toBe(ext.toLowerCase());
        expect(ext.startsWith('.')).toBeFalse();
        expect(seen.has(ext)).withContext(`${ext} listed twice`).toBeFalse();
        seen.add(ext);
      }
    }
  });

  it('classifies like the engine: last extension, any case, unknown → other', () => {
    expect(categoryOf('ubuntu-24.04.iso')).toBe('compressed');
    expect(categoryOf('backup.tar.gz')).toBe('compressed');
    expect(categoryOf('التقرير.PDF')).toBe('documents');
    expect(categoryOf('song.FLAC')).toBe('music');
    expect(categoryOf('movie.mkv')).toBe('video');
    expect(categoryOf('setup.exe')).toBe('programs');
    expect(categoryOf('photo.jpeg')).toBe('images');
    expect(categoryOf('README')).toBe('other');
    expect(categoryOf('weird.xyz')).toBe('other');
  });
});
