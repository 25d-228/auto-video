#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Local helper for the auto-video prototype (no API keys).
   The browser (localhost:8900) calls this same-machine helper, which fetches torrent
   listings + metadata + covers server-side using the user's own IP — bypassing the
   browser's CORS wall and any hotlink/Referer requirements. Endpoints:
     /discover?cat=mov|tv|ad|vrc&mode=trending|newest&n=50  -> live feed (real items)
     /img?u=<url>           -> image passthrough (adds Referer for flaky hosts)
     /meta?cid=&code=&cat=  -> Japanese title + cast (r18.dev/javdb), paced + cached
     /open?path=  /reveal?path=  /scan?path=&cat=
   Real sources verified live: YTS (movies), apibay/TPB (TV) + TVmaze covers,
   sukebei.nyaa.si HTML (adult/VR) + FANZA/DMM covers."""
import sys, json, os, time, threading, re, ssl, gzip, zlib, struct, string, collections, subprocess, hashlib, base64
import html as _htmlmod
import urllib.request, urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from concurrent.futures import ThreadPoolExecutor

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8902
HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, "proxy_cache.json")
COVERCACHE = os.path.join(HERE, "discover_covers.json")
PATHS_FILE = os.path.join(HERE, "av_paths.json")
KEYS_FILE = os.path.join(HERE, "av_keys.json")
ctx = ssl.create_default_context(); ctx.check_hostname=False; ctx.verify_mode=ssl.CERT_NONE
UA = {'User-Agent':'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36'}
EXEC = ThreadPoolExecutor(max_workers=8)
LIST_TTL = 300  # seconds to cache a listing before re-fetching

try: cache = json.load(open(CACHE, encoding="utf-8"))
except Exception: cache = {}
try: covercache = json.load(open(COVERCACHE, encoding="utf-8"))
except Exception: covercache = {}
try: paths_store = json.load(open(PATHS_FILE, encoding="utf-8"))
except Exception: paths_store = {}
try: keys_store = json.load(open(KEYS_FILE, encoding="utf-8"))
except Exception: keys_store = {}
clock = threading.Lock(); last = [0.0]      # pace r18.dev calls
tv_lock = threading.Lock(); tv_last = [0.0] # pace tvmaze calls
cc_lock = threading.Lock()
list_lock = threading.Lock(); _listcache = {}

# ----------------------------------------------------------------- http helpers
def _decompress(b, enc):
    enc = (enc or '').lower()
    if enc == 'gzip' or b[:2] == b'\x1f\x8b':
        try: return gzip.decompress(b)
        except Exception: return b
    if enc == 'deflate':
        try: return zlib.decompress(b)
        except Exception:
            try: return zlib.decompress(b, -zlib.MAX_WBITS)
            except Exception: return b
    return b

def http_get(url, ref=None, timeout=20):
    h = dict(UA); h['Accept-Encoding'] = 'gzip, deflate'
    if ref: h['Referer'] = ref
    r = urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=timeout, context=ctx)
    return _decompress(r.read(), r.headers.get('Content-Encoding')), r

def get_text(url, ref=None):
    try:
        b, _ = http_get(url, ref); return b.decode('utf-8', 'replace')
    except Exception: return ''

def get_json(url, ref=None):
    try:
        b, _ = http_get(url, ref); return json.loads(b.decode('utf-8', 'replace'))
    except Exception: return None

def _unescape(s): return _htmlmod.unescape(s or '')

# resolve a Browse-picked folder (name only) to its absolute path by scanning the
# fixed drives (BFS, shallow-first, time-budgeted) for a dir of that name that
# contains a known file from the import.
find_lock = threading.Lock()
_SKIP = {'windows', 'program files', 'program files (x86)', 'programdata', '$recycle.bin',
         'system volume information', 'perflogs', 'recovery', 'msocache', 'appdata',
         'intel', 'amd', 'nvidia', 'node_modules', 'config.msi'}

def fixed_drives():
    out = []
    for L in string.ascii_uppercase:
        d = L + ':\\'
        if os.path.exists(d): out.append(d)
    return out

def _has_file(root, probe, max_depth=3, budget=4.0):
    if not probe: return True
    t0 = time.time()
    q = collections.deque([(root, 0)])
    while q:
        if time.time() - t0 > budget: return False
        p, d = q.popleft()
        try: entries = list(os.scandir(p))
        except Exception: continue
        for e in entries:
            try:
                if e.is_file(follow_symlinks=False):
                    if e.name == probe: return True
                elif d + 1 < max_depth and not e.name.startswith('$'):
                    q.append((e.path, d + 1))
            except Exception: continue
    return False

def find_folder(name, probe, budget=15.0, max_depth=6):
    name_l = (name or '').lower()
    if not name_l: return ''
    t0 = time.time()
    q = collections.deque((d, 0) for d in fixed_drives())
    while q:
        if time.time() - t0 > budget: return ''
        path, depth = q.popleft()
        try: entries = list(os.scandir(path))
        except Exception: continue
        for e in entries:
            try:
                if not e.is_dir(follow_symlinks=False): continue
            except Exception: continue
            nm = e.name
            if nm.startswith('$') or nm.lower() in _SKIP: continue
            if nm.lower() == name_l and _has_file(e.path, probe):
                return e.path
            if depth + 1 < max_depth: q.append((e.path, depth + 1))
    return ''

def human_size(b):
    b = float(b or 0)
    for u_ in ('B', 'KB', 'MB', 'GB', 'TB'):
        if b < 1024: return ('%d %s' % (int(b), u_)) if u_ == 'B' else ('%.1f %s' % (b, u_))
        b /= 1024
    return '%.1f PB' % b

# ----------------------------------------------------------------- caches
def listing_cached(key, fn, fresh=False):
    if not fresh:
        with list_lock:
            e = _listcache.get(key)
            if e and (time.time() - e[0]) < LIST_TTL:
                return [dict(x) for x in e[1]]
    data = fn() or []
    with list_lock: _listcache[key] = (time.time(), data)
    return [dict(x) for x in data]

def cover_cached(key, fn):
    with cc_lock:
        if key in covercache: return covercache[key]
    v = ''
    try: v = fn() or ''
    except Exception: v = ''
    with cc_lock:
        covercache[key] = v
        try: json.dump(covercache, open(COVERCACHE, 'w', encoding='utf-8'), ensure_ascii=False)
        except Exception: pass
    return v

# ----------------------------------------------------------------- covers
def _img(url):
    return "http://127.0.0.1:%d/img?u=%s" % (PORT, urllib.parse.quote(url, safe='')) if url else ''

DMM_ALIAS = {'ebon': 'ebod'}  # printed-label -> FANZA cid label (extend over time)
# Known FANZA maker prefixes (the h_NNNN before the label) that are NOT derivable from the
# code. Small maintained table — the same mechanism MetaTube/javdatabase rely on. Extend over time.
DMM_PREFIX = {'ccvr': 'h_1270', 'devr': 'h_1711', 'clot': 'h_237'}

def dmm_cid_variants(code):
    m = re.match(r'^(\d*[A-Za-z]+)-?(\d+)$', code or '')  # allow leading digit (3DSVR)
    if not m: return []
    lab = m.group(1).lower(); num = m.group(2)
    lab = DMM_ALIAS.get(lab, lab)
    out = [lab + num.zfill(5), lab + num.zfill(3), lab + num,
           '1' + lab + num.zfill(5), '1' + lab + num.zfill(3),   # FANZA prepends 1 to e.g. 3DSVR
           '13' + lab + num.zfill(5), '13' + lab + num.zfill(3)]  # FANZA VR: DSVR -> 13dsvr01911
    pre = DMM_PREFIX.get(lab)
    if pre:  # known maker prefix (h_NNNN) takes priority
        out = [pre + lab + num.zfill(5), pre + lab + num.zfill(3)] + out
    return list(dict.fromkeys(out))

def img_dims(b):
    if not b or len(b) < 24: return None
    if b[:2] == b'\xff\xd8':                                   # JPEG: scan SOF markers
        i, n = 2, len(b)
        while i < n - 9:
            if b[i] != 0xFF: i += 1; continue
            mk = b[i + 1]
            if 0xC0 <= mk <= 0xCF and mk not in (0xC4, 0xC8, 0xCC):
                return (struct.unpack('>H', b[i + 7:i + 9])[0], struct.unpack('>H', b[i + 5:i + 7])[0])
            seg = struct.unpack('>H', b[i + 2:i + 4])[0]; i += 2 + seg
        return None
    if b[:8] == b'\x89PNG\r\n\x1a\n':
        return (struct.unpack('>I', b[16:20])[0], struct.unpack('>I', b[20:24])[0])
    if b[:4] == b'RIFF' and b[8:12] == b'WEBP':
        t = b[12:16]
        if t == b'VP8X': return (1 + (b[24] | b[25] << 8 | b[26] << 16), 1 + (b[27] | b[28] << 8 | b[29] << 16))
        if t == b'VP8 ': return (struct.unpack('<H', b[26:28])[0] & 0x3fff, struct.unpack('<H', b[28:30])[0] & 0x3fff)
        if t == b'VP8L':
            bits = b[21] | b[22] << 8 | b[23] << 16 | b[24] << 24
            return ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
    return None

def _cover_meta(url, ref):
    # GET the image, reject "now printing" placeholders, return (ok, aspect_ratio w/h).
    try:
        b, _ = http_get(url, ref, timeout=10)
    except Exception:
        return (False, 0)
    if len(b) < 6000: return (False, 0)             # ps placeholder (~3.4KB)
    d = img_dims(b)
    if d and d == (590, 800): return (False, 0)     # pl "now printing" placeholder (~19KB, 590x800)
    ar = round(d[0] / d[1], 3) if (d and d[1]) else 0.72
    return (True, ar)

DMM_FLOORS = ('digital/video', 'digital/amateur')   # studio video AND amateur floor (POW etc.)
DMM_SUFFIXES = ('ps', 'jp', 'pl')                    # front portrait, amateur jacket, wide jacket
def dmm_cover(code):
    for cid in dmm_cid_variants(code):
        for floor in DMM_FLOORS:
            for suf in DMM_SUFFIXES:
                url = "https://pics.dmm.co.jp/%s/%s/%s%s.jpg" % (floor, cid, cid, suf)
                ok, ar = _cover_meta(url, "https://www.dmm.co.jp/")
                if ok: return (url, ar)
    return ('', 0)

def _pace_tv():
    with tv_lock:
        dt = time.time() - tv_last[0]
        if dt < 0.2: time.sleep(0.2 - dt)
        tv_last[0] = time.time()

def tvmaze_cover(imdb, name):
    if imdb:
        _pace_tv()
        j = get_json("https://api.tvmaze.com/lookup/shows?imdb=%s" % urllib.parse.quote(imdb))
        if isinstance(j, dict) and j.get('image'):
            return j['image'].get('medium') or j['image'].get('original') or ''
    if name:
        _pace_tv()
        j = get_json("https://api.tvmaze.com/singlesearch/shows?q=%s" % urllib.parse.quote(name))
        if isinstance(j, dict) and j.get('image'):
            return j['image'].get('medium') or j['image'].get('original') or ''
    return ''

# ----------------------------------------------------------------- jav parsing
VR_LABELS = re.compile(r'\b(SIVR|IPVR|DSVR|CRVR|VRKM|3DSVR|VRTM|EXVR|KAVR|TMAVR|MAXVR|AJVR|JUVR|HNVR|WPVR|TPVR|DOVR|SAVR|VDVR|MDVR|VOVS|CBIKMV|URVRSP|KMVR|FSVSS)\b', re.I)

def is_vr(title, code):
    t = title or ''
    if re.search(r'(^|[\s\[\(])VR([\s\]\)]|専用|$)', t): return True
    if '[VR]' in t.upper(): return True
    if code and VR_LABELS.search(code): return True
    return False

def parse_code(title):
    t = title or ''
    mu = re.search(r'FC2[-\s_]?PPV[-\s_]?(\d{6,7})', t, re.I)
    if mu: return 'FC2-PPV-' + mu.group(1)
    # label may carry a leading digit (e.g. 3DSVR); keep it only for known VR labels
    m = re.search(r'(\d?[A-Za-z]{2,6})[-_\s]?(\d{2,5})', t)
    if not m: return ''
    lab = m.group(1).upper(); num = m.group(2)
    if lab[:1].isdigit() and not VR_LABELS.search(lab):
        lab = lab[1:]
    return lab + '-' + num

def parse_tv_name(name):
    s = name or ''
    m = re.search(r'\bS(\d{1,2})E(\d{1,3})\b', s, re.I)
    se = 'S%02dE%02d' % (int(m.group(1)), int(m.group(2))) if m else ''
    markers = [r'\bS\d{1,2}E\d{1,3}\b', r'\bS\d{1,2}\b', r'\bSeason\s*\d+', r'\b(19|20)\d{2}\b',
               r'\b\d{3,4}p\b', r'\bx26[45]\b', r'\bWEB', r'\bBluRay', r'\bHDTV', r'\bDVDRip',
               r'\bComplete', r'\bREPACK', r'\bHEVC', r'\b720|\b1080|\b2160']
    cut = len(s)
    for mk in markers:
        mm = re.search(mk, s, re.I)
        if mm and mm.start() < cut: cut = mm.start()
    series = re.sub(r'[._]', ' ', s[:cut])
    series = re.sub(r'\s+', ' ', series).strip(' -:.[]')
    return (series or s), se

# ----------------------------------------------------------------- listings
YTS_BASES = ("https://yts.bz/api/v2/", "https://movies-api.accel.li/api/v2/",
             "https://yts.lt/api/v2/", "https://yts.mx/api/v2/")

def fetch_movies(mode):
    sort = 'date_added' if mode == 'newest' else 'download_count'
    movies = []
    for page in (1, 2):  # YTS caps limit at 50, so 2 pages -> up to 100
        got = None
        for base in YTS_BASES:
            j = get_json(base + "list_movies.json?limit=50&page=%d&sort_by=%s&order_by=desc" % (page, sort))
            if isinstance(j, dict) and j.get('status') == 'ok' and j.get('data') and j['data'].get('movies'):
                got = j['data']['movies']; break
        if not got: break
        movies += got
    if not movies: return []
    out = []
    for m in movies:
        tors = m.get('torrents') or []
        seeds = max([int(t.get('seeds') or 0) for t in tors] or [0])
        size = (tors[0].get('size') if tors else '') or ''
        rt = int(m.get('runtime') or 0); yr = m.get('year') or ''
        sub = ("%s · %dh %02dm" % (yr, rt // 60, rt % 60)) if rt else str(yr)
        out.append({'id': 'mov_%s' % m.get('id'), 'cat': 'mov',
                    'title': m.get('title') or m.get('title_long') or '',
                    'sub': sub, 'cover': _img(m.get('large_cover_image') or ''), 'ar': 0.675,
                    'seeders': seeds, 'size': size, 'src': 'YTS', 'state': 'new',
                    'year': yr, 'runtime': rt, 'rating': m.get('rating') or 0,
                    'code': m.get('imdb_code') or ''})
    return out

def fetch_tv(mode):
    if mode == 'newest':
        arr = get_json("https://apibay.org/q.php?q=category:205")
    else:
        arr = get_json("https://apibay.org/precompiled/data_top100_205.json")
    if not isinstance(arr, list): return []
    arr = [x for x in arr if str(x.get('id') or '0') != '0' and (x.get('name') or '')
           and x.get('name') != 'No results returned']
    for x in arr:
        try: x['_seed'] = int(x.get('seeders') or 0)
        except Exception: x['_seed'] = 0
    if mode != 'newest':
        arr.sort(key=lambda x: x['_seed'], reverse=True)
    out = []
    for x in arr[:100]:
        name = x.get('name') or ''; imdb = (x.get('imdb') or '').strip()
        series, se = parse_tv_name(name)
        try: size = human_size(int(x.get('size') or 0))
        except Exception: size = ''
        out.append({'id': 'tv_%s' % x.get('id'), 'cat': 'tv', 'title': series or name,
                    '_name': series, 'sub': ((se + ' · ' if se else '') + (imdb or '')).strip(' ·'),
                    'cover': '', 'ar': 0.7, 'seeders': x['_seed'], 'size': size, 'src': 'TPB',
                    'state': 'new', 'year': '', 'runtime': 0, 'rating': 0,
                    'code': imdb, '_imdb': imdb})
    return out

def fetch_sukebei(mode, query='', pages=None):
    sort = 'id' if mode == 'newest' else 'seeders'
    items = []; seen = set()
    if pages is None: pages = (1 if query else 3)
    qs = ('&q=' + urllib.parse.quote(query)) if query else ''
    for p in range(1, pages + 1):
        h = get_text("https://sukebei.nyaa.si/?c=2_2%s&s=%s&o=desc&p=%d" % (qs, sort, p),
                     ref="https://sukebei.nyaa.si/")
        mt = re.search(r'<tbody>(.*?)</tbody>', h, re.S)
        if not mt: break
        rows = re.findall(r'<tr[^>]*>(.*?)</tr>', mt.group(1), re.S)
        if not rows: break
        for r in rows:
            mv = re.search(r'/view/(\d+)"\s+title="([^"]*)"', r) or re.search(r'/view/(\d+)"[^>]*>([^<]+)<', r)
            if not mv: continue
            vid = mv.group(1)
            if vid in seen: continue
            seen.add(vid)
            title = _unescape(mv.group(2))
            mg = re.search(r'href="(magnet:[^"]+)"', r)
            magnet = _unescape(mg.group(1)) if mg else ''
            tds = re.findall(r'<td class="text-center"[^>]*>(.*?)</td>', r, re.S)
            def _num(x):
                try: return int(re.sub(r'[^\d]', '', x))
                except Exception: return 0
            seeders = _num(tds[-3]) if len(tds) >= 3 else 0
            size = re.sub(r'<[^>]+>', '', tds[-5]).strip() if len(tds) >= 5 else ''
            code = parse_code(title)
            items.append({'id': 'sk_%s' % vid, 'cat': None, 'title': code or title[:48],
                          '_rawtitle': title, 'sub': '', 'cover': '', 'ar': 0.72,
                          'seeders': seeders, 'size': size, 'src': 'sukebei', 'state': 'new',
                          'year': '', 'runtime': 0, 'rating': 0, 'code': code,
                          'magnet': magnet, 'vr': is_vr(title, code)})
        time.sleep(0.25)
    items.sort(key=lambda x: x['seeders'], reverse=(sort == 'seeders'))
    return items

# ---------------------------------------------------- per-item REAL seeder aggregation
# Given a Discover item, query the seeder sites that carry its category and return real
# releases (name + size + live seeder count + magnet) merged + sorted. This is what makes
# the Download dialog and the per-item seed badge show real data, not mocked numbers.
TRACKERS = ''.join('&tr=' + urllib.parse.quote(t) for t in (
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://exodus.desync.com:6969/announce"))
def _quality(name):
    n = (name or '').lower()
    for q in ('2160p', '4k', '8k', '1080p', '720p', '480p'):
        if q in n: return q.upper()
    return ''
def _magnet(ih, name):
    return "magnet:?xt=urn:btih:%s&dn=%s%s" % (ih, urllib.parse.quote(name or ''), TRACKERS)
def seeders_apibay(query):
    arr = get_json("https://apibay.org/q.php?q=" + urllib.parse.quote(query))
    out = []
    if isinstance(arr, list):
        for t in arr:
            ih = (t.get('info_hash') or '')
            name = t.get('name') or ''
            if not ih or set(ih) == {'0'} or name == 'No results returned': continue
            try: seeders = int(t.get('seeders') or 0)
            except Exception: seeders = 0
            try: size = human_size(int(t.get('size') or 0))
            except Exception: size = ''
            out.append({'name': name, 'source': 'TPB', 'seeders': seeders, 'size': size,
                        'magnet': _magnet(ih, name), 'quality': _quality(name)})
    return out
def seeders_yts(title, year):
    j = get_json("https://yts.mx/api/v2/list_movies.json?limit=4&query_term=" + urllib.parse.quote(title))
    out = []
    for m in ((j or {}).get('data') or {}).get('movies') or []:
        if year and m.get('year') and str(m.get('year')) != str(year): continue
        for t in (m.get('torrents') or []):
            ih = t.get('hash') or ''
            if not ih: continue
            nm = ("%s [%s %s]" % (m.get('title_long') or title, t.get('quality') or '', t.get('type') or '')).strip()
            out.append({'name': nm, 'source': 'YTS', 'seeders': int(t.get('seeds') or 0),
                        'size': t.get('size') or '', 'magnet': _magnet(ih, nm), 'quality': t.get('quality') or ''})
    return out
def seeders_sukebei(code):
    if not code: return []
    out = []
    for x in fetch_sukebei('trending', query=code, pages=1):
        nm = x.get('_rawtitle') or x.get('title') or ''
        out.append({'name': nm, 'source': 'sukebei', 'seeders': x.get('seeders') or 0,
                    'size': x.get('size') or '', 'magnet': x.get('magnet') or '', 'quality': _quality(nm)})
    return out
def _jb_get(url, ref=None):
    try:
        h = {**UA, 'Cookie': keys_store.get('javbus', ''), 'Accept-Encoding': 'gzip, deflate'}
        if ref: h['Referer'] = ref
        r = urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=15, context=ctx)
        return _decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace')
    except Exception:
        return ''
def seeders_javbus(code):   # javbus lists magnets (sizes, no seeder counts) via a gid/uc ajax
    if not code: return []
    page = _jb_get("https://www.javbus.com/" + urllib.parse.quote(code), ref="https://www.javbus.com/")
    g = re.search(r'gid\s*=\s*(\d+)', page)
    if not g: return []
    u = re.search(r'\buc\s*=\s*(\d+)', page)
    aj = _jb_get("https://www.javbus.com/ajax/uncledatoolsbyajax.php?gid=%s&lang=en&img=&uc=%s&floor=" %
                 (g.group(1), u.group(1) if u else '0'), ref="https://www.javbus.com/" + code)
    out, seen = [], set()   # the ajax repeats each magnet across title/size/date cells — dedup by hash
    for m in re.finditer(r'magnet:\?xt=urn:btih:([0-9a-fA-F]+)[^"\'\s<]*', aj):
        ih = m.group(1).lower()
        if ih in seen: continue
        seen.add(ih)
        seg = aj[m.start(): m.start() + 500]
        szm = re.search(r'(\d+(?:\.\d+)?\s*[GM]B)', seg)
        out.append({'name': code, 'source': 'JavBus', 'seeders': 0, 'size': (szm.group(1) if szm else ''),
                    'magnet': _unescape(m.group(0)), 'quality': _quality(seg)})
    return out
def seeders_javdb(code):   # javdb app API magnets for codes seen in rankings (size in MB, no seeder counts)
    if not code: return []
    mid = JDB_CODE2ID.get((code or '').upper())
    if not mid: return []
    j = jdb_api("/api/v1/movies/%s/magnets" % mid)
    out = []
    for m in ((j or {}).get('data') or {}).get('magnets') or []:
        ih = m.get('hash') or ''
        if not ih: continue
        nm = m.get('name') or code
        try: sz = human_size(int(m.get('size') or 0) * 1048576)
        except Exception: sz = ''
        out.append({'name': nm, 'source': 'javdb', 'seeders': 0, 'size': sz,
                    'magnet': _magnet(ih, nm), 'quality': 'HD' if m.get('hd') else ''})
    return out
def build_seeders(cat, title, code, year):
    rels = []
    if cat in ('ad', 'vrc'):
        rels += seeders_sukebei(code or title)
        for fn in (seeders_javdb, seeders_javbus):
            try: rels += fn(code or title)
            except Exception: pass
    else:
        q = ("%s %s" % (title, year)).strip() if (cat == 'mov' and year) else (title or '')
        rels += seeders_apibay(q)
        if cat == 'mov':
            try: rels += seeders_yts(title, year)
            except Exception: pass
    seen, uniq = set(), []
    for r in rels:
        h = re.search(r'btih:([0-9a-fA-F]+)', r.get('magnet') or '')
        k = (h.group(1).lower() if h else r.get('name'))
        if k in seen: continue
        seen.add(k); uniq.append(r)
    uniq.sort(key=lambda r: r.get('seeders') or 0, reverse=True)
    return uniq

def javdb_cover(code):
    # javdatabase exposes the *correct* DMM cid (incl. h_/118/maker prefixes) + a webp
    # mirror. Prefer DMM ps (portrait), then pl (wide jacket), then the webp.
    h = get_text("https://www.javdatabase.com/movies/%s/" % code.lower())
    if not h or len(h) < 1000: return ('', 0)
    m = re.search(r'https://pics\.dmm\.co\.jp/digital/video/([a-z0-9_]+)/[a-z0-9_]+p[sl]\.jpg', h)
    if m:
        cid = m.group(1)
        for suf in ('ps', 'pl'):
            url = "https://pics.dmm.co.jp/digital/video/%s/%s%s.jpg" % (cid, cid, suf)
            ok, ar = _cover_meta(url, "https://www.dmm.co.jp/")
            if ok: return (url, ar)
    mw = re.search(r'https://www\.javdatabase\.com/covers/[^"\']+?\.webp', h)
    if mw:
        ok, ar = _cover_meta(mw.group(0), "https://www.javdatabase.com/")
        if ok: return (_img(mw.group(0)), ar)      # route webp via /img (hotlink-safe)
    return ('', 0)

def r18_cover(code):
    # r18.dev is UNGATED and stores the exact DMM jacket URL (incl. amateur floor / maker prefix
    # we can't otherwise derive). Paced to respect r18's rate limit.
    j = paced_get_json("https://r18.dev/videos/vod/movies/detail/-/dvd_id=%s/json" % code, "https://r18.dev/")
    if not isinstance(j, dict): return ('', 0)
    ji = (j.get('images') or {}).get('jacket_image') or {}
    for u in ((ji.get('large') or '').strip(), (ji.get('large2') or '').strip()):
        if u:
            ok, ar = _cover_meta(u, "https://www.dmm.co.jp/")
            if ok: return (u, ar)
    return ('', 0)

def javbus_cover(code):
    # Gated index. Uses the user's verified javbus cookie (keys_store['javbus']) to read the
    # product page's <a class="bigImage"> — the cover for h_NNNN titles no free source lists.
    # javbus-hosted covers are hotlink-protected (need a javbus Referer) so route them via /img.
    ck = (keys_store.get('javbus') or '').strip()
    if not ck: return ('', 0)
    html = ''
    for u in ("https://www.javbus.com/%s" % code, "https://www.javbus.com/en/%s" % code):
        try:
            h = dict(UA); h['Accept-Encoding'] = 'gzip, deflate'
            h['Cookie'] = ck; h['Referer'] = 'https://www.javbus.com/'
            r = urllib.request.urlopen(urllib.request.Request(u, headers=h), timeout=20, context=ctx)
            html = _decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace')
        except Exception:
            html = ''
        if html and 'Age Verification' not in html and 'bigImage' in html:
            break
    m = re.search(r'<a class="bigImage"[^>]*href="([^"]+)"', html or '')
    if not m: return ('', 0)
    cov = m.group(1)
    if cov.startswith('//'): cov = 'https:' + cov
    elif cov.startswith('/'): cov = 'https://www.javbus.com' + cov
    jb = 'javbus.com' in cov
    ok, ar = _cover_meta(cov, 'https://www.javbus.com/' if jb else 'https://www.dmm.co.jp/')
    if not ok: return ('', 0)
    return (_img(cov) if jb else cov, ar)   # route javbus-hosted covers through the referer proxy

def mgstage_ids(code):
    # MGStage product ids drop leading zeros (PRVRSS-00007 -> PRVRSS-007). Try common paddings.
    m = re.match(r'^([0-9A-Za-z]+)-?(\d+)$', code or '')
    if not m: return [code]
    lab = m.group(1).upper(); num = m.group(2); n = int(num)
    return list(dict.fromkeys([code, '%s-%03d' % (lab, n), '%s-%d' % (lab, n), '%s-%s' % (lab, num)]))

def mgstage_cover(code):
    # MGStage-exclusive labels (SIRO/LUXU/PRVRSS...). Age cookie adc=1; missing product REDIRECTS
    # (not 404). Package image on image.mgstage.com is hotlink-protected -> route via /img.
    for mid in mgstage_ids(code):
        path = "/product/product_detail/%s/" % mid
        try:
            h = dict(UA); h['Accept-Encoding'] = 'gzip, deflate'
            h['Cookie'] = 'adc=1'; h['Referer'] = 'https://www.mgstage.com/'
            r = urllib.request.urlopen(urllib.request.Request("https://www.mgstage.com" + path, headers=h), timeout=20, context=ctx)
            if path not in r.geturl(): continue          # redirected to home = not found
            html = _decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace')
        except Exception:
            continue
        cands = re.findall(r'https?://image\.mgstage\.com/images/[^"\'\s]+?\.jpg', html)
        big = [x for x in cands if 'pb_e' in x] or [x for x in cands if 'pf_o1' in x] or cands
        for cov in big[:3]:
            ok, ar = _cover_meta(cov, "https://www.mgstage.com/")
            if ok: return (_img(cov), ar)
    return ('', 0)

def jav_cover(code):
    if not code or code.startswith('FC2'): return ('', 0)   # FC2 truly has no studio cover
    r = dmm_cover(code)              # derive cid -> try video+amateur floors, ps/jp/pl suffixes
    if r[0]: return r
    r = r18_cover(code)             # ungated index: exact DMM url incl maker prefix we can't guess
    if r[0]: return r
    r = javbus_cover(code)          # gated index (user cookie): covers for h_NNNN titles
    if r[0]: return r
    r = mgstage_cover(code)         # MGStage-exclusive labels (own CDN; code drops leading zeros)
    if r[0]: return r
    return javdb_cover(code)

def resolve_covers(cat, items):
    if cat == 'tv':
        def fn(x):
            x['cover'] = cover_cached('tv:' + (x.get('_imdb') or x.get('_name') or x['id']),
                                      lambda: tvmaze_cover(x.get('_imdb'), x.get('_name')))
        list(EXEC.map(fn, items))
    elif cat in ('ad', 'vrc'):
        def fn(x):
            r = cover_cached('jav2:' + x['code'], lambda: jav_cover(x['code'])) if x.get('code') else ('', 0)
            x['cover'] = r[0] if r else ''
            x['ar'] = (r[1] if (r and len(r) > 1) else 0) or 0.72
        list(EXEC.map(fn, items))

def _clean(x):
    return {k: v for k, v in x.items() if not k.startswith('_') and k != 'vr'}

# ----------------------------------------------------------------- alt trend sources
# javdb app API (reverse-engineered jdsignature; see memory). Returns rankings with
# code + cover + release date + magnet count in one signed call, bypassing Cloudflare.
JDB_CERT5 = "30820"
JDB_BLOB_A = ("WzE3OCwyMTksMTI3LDE2MSwxODksMTYyLDEyMywxMDMsMTM3LDIxMCwxMjMsMjE5LDE4OSwxNzksMTIzLDIwMiwxMzksMTUwLDEzMywxNjAsMTI2LDIwNywxNjYsMTUxLDE0NiwxNTksMTg4LDEwMCwxMzgsMTM2LDE3NiwxNjEsMTQyLDEwMywxMzUsMTYwLDE0MiwxNzUsMTYwLDEwNCwxMzAsMTIxLDExOCwxMDYsMTMyLDEyNCwxMzAsMTA0LDEzMSwxMjEsMTI2LDE3MywxNDMsMTQwLDEzOCwxMDQsMTMwLDE1OSwxMTgsMTc1LDE0MiwxNTksMTYxLDE1OSwxNDMsMTI0LDEyMywxNjEsMTMxLDEzNywxMzQsMTAxLDEzMSwxNzUsMTU2LDEwMSwxMzEsMTc1LDE1NywxNTcsMTMwLDEzNywxNjAsMTA2LDE0MywxMzcsMTUzLDE2MCwxMzEsMTQwLDEyMiwxMDMsMTQzLDEzNywxMjMsMTU3LDEzMSwxMzcsMTUyLDEwMywxMzIsMTM3LDEyMiwxNzMsMTMwLDE1OSwxMzEsMTU5LDEzMCwxNDAsMTIyLDEwNiwxMzAsMTc1LDEyMywxNTksMTMwLDEyMSwxMzgsMTA0LDEzMiwxMjEsMTM0LDE3NCwxNDMsMTYyLDEyNiwxMDQsMTMwLDEwMywxMjcsMTU3LDEzMCwxMDMsMTI2LDE3NSwxNDIsMTc1LDE1NiwxNzUsMTQyLDE2MiwxMzEsMTYwLDEzMSwxNTksMTYxLDE1OSwxMzAsMTM3LDE1MywxNTksMTQyLDEwMywxNDIsMTczLDEzMSwxNzUsMTM0LDE3MiwxMzIsMTIxLDEyMywxNjEsMTMwLDEwMywxMzQsMTA1LDE0MiwxNDAsMTIyLDExNF0=")
JDB_BLOB_B = "WzE5OCwxNjksMTIzLDEwNiwxNzcsMTY2LDE0MCwxNjIsMTQ3LDE4OSwxNjIsMjE5LDE5OSwxMjIsMTE4LDE1OF0="
def _jdb_dec(key, blob):
    md = hashlib.md5(key.encode()).hexdigest(); last = len(md) - 1
    lst = json.loads(base64.b64decode(blob).decode('utf-8'))
    raw = bytes((v - ord(md[i if i <= last else last])) & 0xff for i, v in enumerate(lst))
    return base64.b64decode(raw).decode('utf-8')
def jdb_signature():
    ts = int(time.time()); a = _jdb_dec(JDB_CERT5, JDB_BLOB_A); b = _jdb_dec(JDB_CERT5, JDB_BLOB_B)
    return "%s.%s.%s" % (ts, b, hashlib.md5(("%s%s" % (ts, a)).encode()).hexdigest())
def jdb_api(path):
    try:
        h = dict(UA); h['Accept-Encoding'] = 'gzip, deflate'; h['jdsignature'] = jdb_signature()
        h['app_version'] = '1.9.35'; h['app_version_number'] = '10935'; h['platform'] = 'android'; h['device'] = 'android'; h['lang'] = 'en'
        r = urllib.request.urlopen(urllib.request.Request("https://jdforrepam.com" + path, headers=h), timeout=15, context=ctx)
        return json.loads(_decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace'))
    except Exception:
        return None
JDB_CODE2ID = {}   # javdb product code -> internal movie id, filled from rankings (for the magnets endpoint)
def fetch_javdb(cat, mode):
    period = 'weekly' if mode == 'trending' else 'daily'
    j = jdb_api("/api/v1/rankings?type=movie&period=%s&category=c" % period)
    movies = ((j or {}).get('data') or {}).get('movies') or []
    out = []
    for i, m in enumerate(movies):
        cov = m.get('cover_url') or m.get('thumb_url') or ''
        if not cov: continue
        date = m.get('release_date') or ''
        if m.get('number') and m.get('id'): JDB_CODE2ID[m['number'].upper()] = m['id']
        out.append({'id': 'jdb_%s' % m.get('id'), 'cat': cat, 'title': m.get('number') or '', 'sub': date,
                    'cover': _img(cov), 'ar': 1.48, 'seeders': m.get('magnets_count', 0), 'size': '',
                    'src': 'javdb', 'state': 'new', 'year': date[:4], 'runtime': m.get('duration', 0),
                    'rating': 0, 'code': m.get('number') or '', 'date': date, 'added': i})
    return out

def fetch_tmdb_trending(kind):
    if not tmdb_key(): return []
    win = 'week' if kind == 'movie' else 'day'
    out = []
    for page in range(1, 6):   # TMDB returns 20/page; pull up to 5 pages (~100) to fill Show 100
        j = _tmdb('trending/%s/%s' % (kind, win), page=page) or {}
        res = j.get('results') or []
        if not res: break
        for m in res:
            poster = m.get('poster_path') or ''
            if not poster: continue
            title = m.get('title') or m.get('name') or ''
            date = m.get('release_date') or m.get('first_air_date') or ''
            out.append({'id': 'tmdbt_%s' % m.get('id'), 'cat': 'mov' if kind == 'movie' else 'tv', 'title': title,
                        'sub': (date[:4] if date else ''), 'cover': TMDB_IMG + poster, 'ar': 0.667, 'seeders': 0,
                        'size': '', 'src': 'TMDB', 'state': 'new', 'year': date[:4], 'runtime': 0,
                        'rating': round(m.get('vote_average') or 0, 1), 'code': str(m.get('id')), 'date': date, 'added': len(out)})
        if page >= (j.get('total_pages') or 1): break
    return out

def fetch_tmdb_list(cat, path):
    if not tmdb_key(): return []
    out = []
    for page in range(1, 6):
        j = _tmdb(path, page=page) or {}
        res = j.get('results') or []
        if not res: break
        for m in res:
            poster = m.get('poster_path') or ''
            if not poster: continue
            title = m.get('title') or m.get('name') or ''
            date = m.get('release_date') or m.get('first_air_date') or ''
            out.append({'id': 'tmdbp_%s' % m.get('id'), 'cat': cat, 'title': title,
                        'sub': (date[:4] if date else ''), 'cover': TMDB_IMG + poster, 'ar': 0.667, 'seeders': 0,
                        'size': '', 'src': 'TMDB', 'state': 'new', 'year': date[:4], 'runtime': 0,
                        'rating': round(m.get('vote_average') or 0, 1), 'code': str(m.get('id')), 'date': date, 'added': len(out)})
        if page >= (j.get('total_pages') or 1): break
    return out

def imdb_gql(query):
    try:
        data = json.dumps({"query": query}).encode()
        r = urllib.request.urlopen(urllib.request.Request("https://api.graphql.imdb.com/", data=data,
            headers={**UA, 'Content-Type': 'application/json'}), timeout=20, context=ctx)
        return json.loads(_decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace'))
    except Exception:
        return None
def fetch_imdb_chart(cat):
    tt = 'movie' if cat == 'mov' else 'tvSeries'
    q = ('query{advancedTitleSearch(first:60,sort:{sortBy:POPULARITY,sortOrder:ASC},'
         'constraints:{titleTypeConstraint:{anyTitleTypeIds:["%s"]}}){edges{node{title{'
         'id titleText{text} releaseYear{year} primaryImage{url} ratingsSummary{aggregateRating}}}}}}') % tt
    j = imdb_gql(q) or {}
    out = []
    for e in ((((j.get('data') or {}).get('advancedTitleSearch')) or {}).get('edges') or []):
        t = (e.get('node') or {}).get('title') or {}
        img = (t.get('primaryImage') or {}).get('url') or ''
        if not img: continue
        yr = (t.get('releaseYear') or {}).get('year') or ''
        out.append({'id': 'imdb_%s' % t.get('id'), 'cat': cat, 'title': (t.get('titleText') or {}).get('text') or '',
                    'sub': str(yr or ''), 'cover': img, 'ar': 0.675, 'seeders': 0, 'size': '', 'src': 'IMDb',
                    'state': 'new', 'year': str(yr or ''), 'runtime': 0,
                    'rating': round((t.get('ratingsSummary') or {}).get('aggregateRating') or 0, 1),
                    'code': t.get('id') or '', 'date': '', 'added': len(out)})
    return out

def _mg_get(url):
    try:
        h = dict(UA); h['Cookie'] = 'adc=1'; h['Referer'] = 'https://www.mgstage.com/'; h['Accept-Encoding'] = 'gzip, deflate'
        r = urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=15, context=ctx)
        return _decompress(r.read(), r.headers.get('Content-Encoding')).decode('utf-8', 'replace')
    except Exception:
        return ''
def fetch_mgstage(vr, mode):
    if vr:
        html = _mg_get("https://www.mgstage.com/search/search.php?search_word=VR&sort=popular&type=top")
    else:
        html = _mg_get("https://www.mgstage.com/ranking/ranking.php?id=%s" % ('week' if mode == 'trending' else 'day'))
    out = []; seen = set()
    # each product block: a product_detail link near an image.mgstage.com cover
    for m in re.finditer(r'/product/product_detail/([0-9A-Za-z_-]+)/"[\s\S]{0,400}?(https?://image\.mgstage\.com/images/[^"\']+?\.jpg)', html or ''):
        code = m.group(1).upper(); cov = m.group(1) and m.group(2)
        if code in seen: continue
        seen.add(code)
        out.append({'id': 'mg_%s' % code, 'cat': 'vrc' if vr else 'ad', 'title': code, 'sub': ('VR' if vr else ''),
                    'cover': _img(cov), 'ar': (0.72 if vr else 0.7), 'seeders': 0, 'size': '', 'src': 'MGStage',
                    'state': 'new', 'year': '', 'runtime': 0, 'rating': 0, 'code': code, 'added': len(out),
                    'vr': vr})
    return out

# ----------------------------------------------------------------- library folder scan
VIDEO_EXT = ('.mkv', '.mp4', '.avi', '.wmv', '.m4v', '.ts', '.mov', '.flv', '.iso', '.rmvb', '.webm', '.mpg', '.mpeg')
def _movie_title(fn):
    base = re.sub(r'[\._]+', ' ', os.path.splitext(fn)[0]).strip()
    ym = re.search(r'\b((?:19|20)\d{2})\b', base)
    year = ym.group(1) if ym else ''
    title = (base[:ym.start()] + ' ' + base[ym.end():]) if ym else base   # drop the year token wherever it sits
    title = re.split(r'(?i)\b(1080p|720p|2160p|480p|bluray|web[\.\-]?dl|webrip|x264|x265|hevc|bdrip|remux|dvdrip)\b', title, 1)[0]
    title = re.sub(r'\s+', ' ', title).strip(' -[](){}')
    return (title or base), year
def scan_library():
    items = []
    for cat in ('mov', 'tv', 'ad', 'vrc'):
        folder = (paths_store.get(cat) or '').strip()
        if not folder or not os.path.isdir(folder):
            continue
        for root, dirs, files in os.walk(folder):
            for fn in files:
                if fn.startswith('.'):          # skip macOS ._AppleDouble + dotfiles
                    continue
                if os.path.splitext(fn)[1].lower() not in VIDEO_EXT:
                    continue
                full = os.path.join(root, fn)
                try: size = os.path.getsize(full)
                except Exception: size = 0
                it = {'fname': fn, 'path': full, 'cat': cat, 'size': human_size(size),
                      'state': 'own', 'cover': '', 'ar': 0.72}
                if cat in ('ad', 'vrc'):
                    code = parse_code(fn)
                    it['code'] = code
                    it['title'] = code or os.path.splitext(fn)[0][:42]
                    it['vr'] = (cat == 'vrc') or is_vr(fn, code)
                    it['sub'] = (('VR · ' if it['vr'] else '') + code).strip(' ·')
                elif cat == 'tv':
                    base = os.path.splitext(fn)[0]
                    series, se = parse_tv_name(base)
                    raw = series if (series and len(series) >= 2) else base
                    clean = re.sub(r'[\._]+', ' ', raw)
                    cut = re.split(r'(?i)\s*(?:#\s*\d+|s\d{1,2}e\d{1,2}|第\d*話|\bep?\s*\d+\b|\bop\b|\bed\b|creditless|ncop|nced|「|\[|\bseason\b|\bvol\b)', clean, 1)[0]
                    cut = re.sub(r'(?i)\s+\d{1,3}(\s*(end|fin))?$', '', cut)   # trailing bare episode number
                    cut = cut.strip(' -·[]」【】')
                    it['title'] = cut or clean.strip() or base
                    it['sub'] = se; it['ar'] = 0.7
                else:
                    title, year = _movie_title(fn)
                    it['title'] = title; it['year'] = year; it['sub'] = year; it['ar'] = 0.675
                items.append(it)
    return items

def lib_stats():
    import shutil
    out = {}
    for cat in ('mov', 'tv', 'ad', 'vrc'):
        folder = (paths_store.get(cat) or '').strip()
        info = {'path': folder, 'online': False, 'free': 0, 'total': 0, 'files': 0}
        if folder and os.path.isdir(folder):
            info['online'] = True
            try:
                du = shutil.disk_usage(folder)
                info['free'], info['total'] = du.free, du.total
            except Exception:
                pass
            try:
                n = 0
                for _r, _d, _fs in os.walk(folder):
                    n += sum(1 for f in _fs if os.path.splitext(f)[1].lower() in VIDEO_EXT)
                info['files'] = n
            except Exception:
                pass
        out[cat] = info
    return out

def build_discover(cat, mode, n, fresh=False, source=None):
    # Two-layer Discover: the SOURCE picks the trend list (tmdb/javdb/mgstage/seeders);
    # covers come with it or are resolved; coverless items are dropped.
    src = (source or '').lower()
    key = '%s|%s|%s' % (cat, mode, src or 'def')
    if cat == 'mov':
        if src == 'imdb':         fn = lambda: fetch_imdb_chart('mov')
        elif src == 'tmdb_popular': fn = lambda: fetch_tmdb_list('mov', 'movie/popular')
        elif src == 'tmdb':       fn = lambda: fetch_tmdb_trending('movie')
        else:                     fn = lambda: fetch_movies(mode)
        data = listing_cached(key, fn, fresh)
        return [_clean(x) for x in [x for x in data if x.get('cover')][:n]]
    if cat == 'tv':
        if src in ('tmdb', 'tmdb_airing', 'imdb'):
            if src == 'imdb':        fn = lambda: fetch_imdb_chart('tv')
            elif src == 'tmdb_airing': fn = lambda: fetch_tmdb_list('tv', 'tv/on_the_air')
            else:                    fn = lambda: fetch_tmdb_trending('tv')
            data = listing_cached(key, fn, fresh)
            return [_clean(x) for x in [x for x in data if x.get('cover')][:n]]
        scan = listing_cached(key, lambda: fetch_tv(mode), fresh)
        resolve_covers('tv', scan)
        return [_clean(x) for x in [x for x in scan if x.get('cover')][:n]]
    # adult / VR
    want_vr = (cat == 'vrc')
    if src == 'javdb' and not want_vr:
        def _jdb():   # javdb's cmastd covers are encrypted (unrenderable) -> resolve a real cover by code
            data = fetch_javdb('ad', mode); resolve_covers(cat, data); return data
        data = listing_cached(key, _jdb, fresh)
        return [_clean(x) for x in [x for x in data if x.get('cover')][:n]]
    if src == 'mgstage':
        def _mg():    # MGStage covers are wide jackets -> resolve a portrait front cover by code
            data = fetch_mgstage(want_vr, mode); resolve_covers(cat, data); return data
        data = listing_cached(key, _mg, fresh)
        return [_clean(x) for x in [x for x in data if x.get('cover')][:n]]
    if want_vr:
        pool = listing_cached(key, lambda: fetch_sukebei(mode, 'VR', pages=4), fresh)
    else:
        pool = listing_cached(key, lambda: fetch_sukebei(mode, '', pages=8), fresh)
    pool = [x for x in pool if bool(x.get('vr')) == want_vr]
    resolve_covers(cat, pool)
    out = []
    for x in pool:
        if not x.get('cover'): continue
        x['cat'] = cat
        x['sub'] = (('VR · ' if want_vr else '') + (x.get('code') or '')).strip(' ·') or (x.get('_rawtitle', '')[:30])
        out.append(x)
        if len(out) >= n: break
    return [_clean(x) for x in out]

# ----------------------------------------------------------------- TMDB (movie/TV authority)
# A title-addressable file (movie/TV) is identified the real way: search the TMDB catalog
# for title+year, take the best match, pull full details (runtime/genre/cast), and build
# the poster URL from poster_path. Multilingual, so Chinese/foreign titles resolve too.
TMDB_IMG = "https://image.tmdb.org/t/p/w780"
def tmdb_key():
    return (keys_store.get('tmdb') or os.environ.get('TMDB_KEY') or '').strip()

def _tmdb(path, **params):
    if not tmdb_key(): return None
    params['api_key'] = tmdb_key()
    return get_json("https://api.themoviedb.org/3/%s?%s" % (path, urllib.parse.urlencode(params)))

def _norm(s): return re.sub(r'\W+', '', (s or '').lower(), flags=re.UNICODE)  # keep CJK; strip space/punct
def _title_match(a, b):
    a, b = _norm(a), _norm(b)
    return bool(a) and bool(b) and (a == b or a in b or b in a)

def anilist_cover(title):
    try:
        body = json.dumps({"query": "query($s:String){Media(search:$s,type:ANIME){coverImage{large medium}}}",
                           "variables": {"s": title}}).encode()
        r = urllib.request.urlopen(urllib.request.Request("https://graphql.anilist.co", data=body,
            headers={"Content-Type": "application/json", "User-Agent": UA['User-Agent']}), timeout=12, context=ctx)
        ci = ((json.loads(r.read().decode('utf-8', 'replace')).get("data") or {}).get("Media") or {}).get("coverImage") or {}
        return ci.get("large") or ci.get("medium") or ""
    except Exception:
        return ""

def tmdb_lookup(title, year, tv=False):
    """search -> pick best (title + year guarded) -> details -> poster. Returns metadata dict or None."""
    if not tmdb_key() or not title: return None
    kind = 'tv' if tv else 'movie'
    yk = 'first_air_date_year' if tv else 'year'
    attempts = ([{'query': title, yk: year}] if year else []) + [{'query': title}]
    res = []
    for params in attempts:
        j = _tmdb('search/' + kind, include_adult='false', **params)
        res = (j or {}).get('results') or []
        if res: break
    if not res: return None
    pick = None
    for m in res:                                   # prefer a title+year match
        dt = (m.get('release_date') or m.get('first_air_date') or '')[:4]
        ok_year = (not year) or (dt and abs(int(dt) - int(year)) <= 1)
        names = [m.get('title'), m.get('name'), m.get('original_title'), m.get('original_name')]
        ok_title = any(_title_match(title, x) for x in names)
        if ok_year and ok_title: pick = m; break
    if not pick:                                    # else top popularity hit, only if title roughly matches
        m = res[0]
        if any(_title_match(title, m.get(k)) for k in ('title', 'name', 'original_title', 'original_name')):
            pick = m
    if not pick: return None
    det = _tmdb('%s/%s' % (kind, pick.get('id')), append_to_response='credits') or {}
    rec = {'tmdb_id': pick.get('id')}
    poster = det.get('poster_path') or pick.get('poster_path') or ''
    if poster: rec['cover'] = TMDB_IMG + poster; rec['ar'] = 0.667    # TMDB posters are 2:3
    date = det.get('release_date') or det.get('first_air_date') or pick.get('release_date') or pick.get('first_air_date') or ''
    if date: rec['date'] = date[:10]; rec['year'] = date[:4]
    rt = det.get('runtime') or ((det.get('episode_run_time') or [0]) or [0])[0]
    if rt: rec['runtime'] = str(rt) + ' min'
    genres = [g.get('name') for g in (det.get('genres') or []) if g.get('name')]
    if genres: rec['genre'] = ', '.join(genres[:3])
    cast = [c.get('name') for c in ((det.get('credits') or {}).get('cast') or [])[:5] if c.get('name')]
    if cast: rec['cast'] = ', '.join(cast)
    t_out = det.get('title') or det.get('name') or ''
    if t_out: rec['tmdb_title'] = t_out
    if det.get('overview'): rec['overview'] = det['overview']
    return rec

# ----------------------------------------------------------------- metadata (existing)
def paced_get_json(url, ref):
    with clock:
        dt = time.time() - last[0]
        if dt < 2.5: time.sleep(2.5 - dt)
        last[0] = time.time()
    for attempt in range(2):
        try:
            h = dict(UA); h['Referer'] = ref; h['Accept'] = 'application/json'
            b, _ = http_get(url, ref, timeout=15)
            return json.loads(b.decode('utf-8', 'replace'))
        except Exception:
            time.sleep(3)
    return None

def from_r18(cid):
    j = paced_get_json("https://r18.dev/videos/vod/movies/detail/-/combined=%s/json" % cid, "https://r18.dev/")
    if not j or not isinstance(j, dict): return None
    rec = {}
    if j.get('title_ja'): rec['jatitle'] = j['title_ja']
    if j.get('release_date'): rec['date'] = str(j['release_date'])[:10]
    if j.get('runtime_mins'): rec['runtime'] = str(j['runtime_mins']) + ' min'
    acts = [a.get('name_kanji') or a.get('name_kana') or a.get('name_romaji') for a in (j.get('actresses') or [])]
    acts = [a for a in acts if a]
    if acts: rec['cast_ja'] = ', '.join(acts)
    return rec or None

def from_javdb(code):
    h = get_text("https://www.javdatabase.com/movies/%s/" % code.lower())
    if not h or len(h) < 2000: return None
    rec = {}
    t = re.search(r'<title>\s*' + re.escape(code) + r'\s*-\s*(.+?)\s*-\s*JAV Database', h, re.I)
    if t and 'jav' not in t.group(1).lower(): rec['cast'] = t.group(1).strip()
    d = re.search(r'(\d{4}-\d{2}-\d{2})', h)
    if d: rec['date'] = d.group(1)
    r = re.search(r'(\d{2,3})\s*min', h, re.I)
    if r: rec['runtime'] = r.group(1) + ' min'
    return rec or None

def recycle(path):
    """Send a file/folder to the Windows Recycle Bin (recoverable). True on success."""
    try:
        import ctypes
        from ctypes import wintypes
        class SHFILEOPSTRUCTW(ctypes.Structure):
            _fields_ = [("hwnd", wintypes.HWND), ("wFunc", wintypes.UINT),
                        ("pFrom", wintypes.LPCWSTR), ("pTo", wintypes.LPCWSTR),
                        ("fFlags", ctypes.c_uint16), ("fAnyOperationsAborted", wintypes.BOOL),
                        ("hNameMappings", ctypes.c_void_p), ("lpszProgressTitle", wintypes.LPCWSTR)]
        FO_DELETE = 3; FOF_ALLOWUNDO = 0x40; FOF_NOCONFIRMATION = 0x10; FOF_SILENT = 0x4; FOF_NOERRORUI = 0x400
        op = SHFILEOPSTRUCTW(); op.hwnd = None; op.wFunc = FO_DELETE
        op.pFrom = os.path.abspath(path) + '\x00\x00'; op.pTo = None
        op.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI
        res = ctypes.windll.shell32.SHFileOperationW(ctypes.byref(op))
        return res == 0 and not op.fAnyOperationsAborted
    except Exception:
        return False

# ----------------------------------------------------------------- server
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, obj, code=200):
        body = json.dumps(obj, ensure_ascii=False).encode('utf-8')
        self.send_response(code)
        self.send_header('Content-Type', 'application/json; charset=utf-8')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Cache-Control', 'no-store')
        self.end_headers(); self.wfile.write(body)
    def do_OPTIONS(self):
        self.send_response(204); self.send_header('Access-Control-Allow-Origin', '*'); self.end_headers()
    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)

        if u.path == '/discover':
            cat = (q.get('cat') or ['mov'])[0]
            mode = (q.get('mode') or ['trending'])[0]
            try: n = max(1, min(100, int((q.get('n') or ['50'])[0] or 50)))
            except Exception: n = 50
            fresh = (q.get('fresh') or ['0'])[0] == '1'
            source = (q.get('source') or [''])[0]
            try:
                items = build_discover(cat, mode, n, fresh, source)
                return self._send({'ok': True, 'items': items, 'count': len(items), 'updated': 'just now', 'source': source})
            except Exception as e:
                return self._send({'ok': False, 'err': str(e)[:160]})

        if u.path == '/seeders':
            cat = (q.get('cat') or ['mov'])[0]
            title = (q.get('title') or [''])[0]
            code = (q.get('code') or [''])[0]
            year = (q.get('year') or [''])[0]
            fresh = (q.get('fresh') or ['0'])[0] == '1'
            key = 'seed|%s|%s|%s' % (cat, (code or title).lower(), year)
            try:
                rels = listing_cached(key, lambda: build_seeders(cat, title, code, year), fresh)
                top = max([r['seeders'] for r in rels] or [0])
                srcs = {}
                for r in rels: srcs[r['source']] = srcs.get(r['source'], 0) + 1
                return self._send({'ok': True, 'releases': rels[:25], 'count': len(rels),
                                   'topSeed': top, 'totalSeed': sum(r['seeders'] for r in rels), 'sources': srcs})
            except Exception as e:
                return self._send({'ok': False, 'err': str(e)[:160]})

        if u.path == '/img':
            src = (q.get('u') or [''])[0]
            if not src: return self._send({'ok': False, 'err': 'no url'}, 400)
            if 'dmm.co.jp' in src: ref = 'https://www.dmm.co.jp/'
            elif 'javdatabase' in src: ref = 'https://www.javdatabase.com/'
            elif 'javbus' in src: ref = 'https://www.javbus.com/'
            elif 'mgstage' in src: ref = 'https://www.mgstage.com/'
            elif 'cmastd' in src or 'javdb' in src: ref = 'https://javdb.com/'
            elif 'yts' in src: ref = 'https://yts.mx/'
            else: ref = None
            try:
                b, r = http_get(src, ref, timeout=15)
                ct = r.headers.get('Content-Type', 'image/jpeg')
                if not ct.startswith('image/'): ct = 'image/jpeg'   # some CDNs send octet-stream
                self.send_response(200)
                self.send_header('Content-Type', ct)
                self.send_header('Access-Control-Allow-Origin', '*')
                self.send_header('Cache-Control', 'public, max-age=86400')
                self.end_headers(); self.wfile.write(b)
            except Exception:
                try:
                    self.send_response(502); self.send_header('Access-Control-Allow-Origin', '*'); self.end_headers()
                except Exception: pass
            return

        if u.path == '/paths':       # restore saved library folder paths (survives relaunch)
            return self._send(paths_store)
        if u.path == '/stats':       # real disk usage + file counts per library folder
            try:
                return self._send({'ok': True, 'disks': lib_stats()})
            except Exception as e:
                return self._send({'ok': False, 'err': str(e)[:160]})
        if u.path == '/library':     # scan the configured folders for real video files
            try:
                items = scan_library()
                cnt = {}
                for it in items:
                    cnt[it['cat']] = cnt.get(it['cat'], 0) + 1
                return self._send({'ok': True, 'items': items, 'count': len(items), 'counts': cnt})
            except Exception as e:
                return self._send({'ok': False, 'err': str(e)[:160]})
        if u.path == '/savepath':    # persist one category's folder path to disk
            cat = (q.get('cat') or [''])[0]; p = (q.get('path') or [''])[0]
            if cat:
                if p: paths_store[cat] = p
                else: paths_store.pop(cat, None)
                try: json.dump(paths_store, open(PATHS_FILE, 'w', encoding='utf-8'), ensure_ascii=False)
                except Exception: pass
            return self._send({'ok': True})

        if u.path == '/findpath':    # locate a Browse-picked folder's absolute path on disk
            name = (q.get('name') or [''])[0]; probe = (q.get('probe') or [''])[0]
            cat = (q.get('cat') or [''])[0]
            with find_lock:
                p = find_folder(name, probe)
            if p and cat:
                paths_store[cat] = p
                try: json.dump(paths_store, open(PATHS_FILE, 'w', encoding='utf-8'), ensure_ascii=False)
                except Exception: pass
            return self._send({'ok': bool(p), 'path': p})

        if u.path == '/cover':
            code = (q.get('code') or [''])[0]
            if not code: return self._send({'ok': False})
            if (q.get('fresh') or ['0'])[0] == '1':
                with cc_lock: covercache.pop('jav2:' + code, None)
            r = cover_cached('jav2:' + code, lambda: jav_cover(code))
            url = r[0] if r else ''
            return self._send({'ok': bool(url), 'cover': url, 'ar': (r[1] if (r and len(r) > 1) else 0)})

        if u.path == '/movie' or u.path == '/tv':   # title-addressable lookup via TMDB
            title = (q.get('title') or [''])[0].strip()
            year = (q.get('year') or [''])[0].strip()
            tv = (u.path == '/tv')
            if not tmdb_key(): return self._send({'ok': False, 'haskey': False})
            if not title: return self._send({'ok': False, 'haskey': True})
            fresh = (q.get('fresh') or ['0'])[0] == '1'
            ck = ('tmdbtv:' if tv else 'tmdb:') + title.lower() + '|' + year
            if fresh:
                with cc_lock: covercache.pop(ck, None)
            rec = cover_cached(ck, lambda: tmdb_lookup(title, year, tv) or {})
            if tv and not (rec and rec.get('cover')):   # anime fallback: AniList (no key needed)
                def _al():
                    c = anilist_cover(title)
                    return {'cover': c, 'ar': 0.69} if c else {}
                ac = cover_cached('anilist:' + title.lower(), _al)
                if ac and ac.get('cover'):
                    rec = ac
            return self._send({'ok': bool(rec and rec.get('cover')), 'haskey': True, 'meta': rec or {}})

        if u.path == '/keys':                       # restore saved provider keys (localhost-only, mirrors /paths)
            return self._send(keys_store)
        if u.path == '/savekey':                    # persist a provider key to disk
            prov = (q.get('provider') or ['tmdb'])[0]; val = (q.get('key') or [''])[0].strip()
            if val: keys_store[prov] = val
            else: keys_store.pop(prov, None)
            try: json.dump(keys_store, open(KEYS_FILE, 'w', encoding='utf-8'), ensure_ascii=False)
            except Exception: pass
            return self._send({'ok': True, 'tmdb': bool(tmdb_key())})

        if u.path == '/open':
            p = (q.get('path') or [''])[0]
            if p and os.path.exists(p):
                try: os.startfile(p); return self._send({'ok': True})
                except Exception as e: return self._send({'ok': False, 'err': str(e)[:90]})
            return self._send({'ok': False, 'err': 'not found: ' + p})
        if u.path == '/reveal':
            p = (q.get('path') or [''])[0]
            if p and os.path.exists(p):
                try: subprocess.Popen('explorer /select,"%s"' % p); return self._send({'ok': True})
                except Exception as e: return self._send({'ok': False, 'err': str(e)[:90]})
            return self._send({'ok': False, 'err': 'not found: ' + p})
        if u.path == '/delete':      # move a library file/folder to the Recycle Bin (recoverable)
            p = (q.get('path') or [''])[0]
            if not p or not os.path.exists(p): return self._send({'ok': False, 'err': 'not found'})
            pn = os.path.normcase(os.path.abspath(p))
            bases = [os.path.normcase(os.path.abspath(b)).rstrip('\\/') for b in paths_store.values() if b]
            if not any(pn == b or pn.startswith(b + os.sep) for b in bases):  # safety: library folders only
                return self._send({'ok': False, 'err': 'outside library folders'})
            ok = recycle(p)
            return self._send({'ok': ok, 'recycled': ok})
        if u.path == '/scan':
            base = (q.get('path') or [''])[0]
            if not base or not os.path.isdir(base): return self._send({'ok': False, 'err': 'not a folder: ' + base})
            ext = re.compile(r'\.(mp4|mkv|avi|wmv|mov|m4v|ts|flv|webm|mpg|mpeg)$', re.I)
            files = []
            for root, _dirs, fs in os.walk(base):
                for f in fs:
                    if f.startswith('._'): continue
                    if ext.search(f): files.append(f)
                if len(files) > 5000: break
            return self._send({'ok': True, 'path': base, 'files': files})

        if u.path != '/meta': self._send({'ok': True}); return
        cid = (q.get('cid') or [''])[0]; code = (q.get('code') or [''])[0]
        key = cid or code
        if not key: self._send({}); return
        if key in cache: self._send(cache[key]); return
        rec = (from_r18(cid) if cid else None) or (from_javdb(code) if code else None) or {}
        cache[key] = rec
        try: json.dump(cache, open(CACHE, 'w', encoding='utf-8'), ensure_ascii=False)
        except Exception: pass
        self._send(rec)

print("auto-video helper on http://127.0.0.1:%d  (meta cache: %d, cover cache: %d)" % (PORT, len(cache), len(covercache)))
ThreadingHTTPServer(('127.0.0.1', PORT), H).serve_forever()
