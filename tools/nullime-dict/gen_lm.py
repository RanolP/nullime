"""Builds the engine's embedded language-model tables (docs/architecture.md "언어 모델").

    uv run python tools/nullime-dict/gen_lm.py --cache <dir>

Downloads the corpora into <dir> (kept out of the repository) unless they are
already there, prints their SHA-256, and writes
crates/nullime-engine/data/ko_syllable.bin, ko_particle.txt and
en_trigram.bin.
Standard library only.
"""

import argparse
import bz2
import collections
import hashlib
import math
import pathlib
import re
import urllib.request
import zipfile

SOURCES = {
    "kor_sentences.tsv.bz2": "https://downloads.tatoeba.org/exports/per_language/kor/kor_sentences.tsv.bz2",
    "eng_sentences.tsv.bz2": "https://downloads.tatoeba.org/exports/per_language/eng/eng_sentences.tsv.bz2",
    # 국립국어원 「현대 국어 사용 빈도 조사」 결과 파일(텍스트 파일), 2002.
    "nikl_freq_2002.zip": "https://www.korean.go.kr/common/download.do?file_path=etcData"
    "&c_file_name=0907a2ca-3391-47e8-8216-8867c22add5a_0.zip&o_file_name=freq.zip",
}

# SHA-256 of the downloads the committed tables were built from. Tatoeba
# re-exports weekly, so a fresh download differs; pass --update to accept it.
EXPECTED = {
    "kor_sentences.tsv.bz2": "453d31fcd3e0b70b113f18048d143352dc2582a5aeb5981c50eb2f0246ef652c",
    "eng_sentences.tsv.bz2": "3b5b3ecfcac5037b09f81ef048fa16fedc4d24838db5b6fbeedca1dfab374cbf",
    "nikl_freq_2002.zip": "fde8b3b47979b7f1273fa423cfe4bfb3298715d831d651ad9fae320e94193d4a",
}

OUT = pathlib.Path(__file__).resolve().parents[2] / "crates/nullime-engine/data"

# Costs are stored as round(-ln p * SCALE) in one byte; the engine divides back.
SCALE = 8.0
# Share of probability mass spread evenly over all 2350 syllables, so ones
# neither corpus saw still beat a syllable outside KS X 1001.
KO_UNIFORM = 0.001
BOUNDARY = 26  # symbol index of the word boundary; a-z are 0..25


def fetch(cache, update):
    cache.mkdir(parents=True, exist_ok=True)
    for name, url in SOURCES.items():
        path = cache / name
        if not path.exists():
            print(f"fetch {url}")
            with urllib.request.urlopen(url) as r:
                path.write_bytes(r.read())
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"{name} sha256 {sha}")
        if sha != EXPECTED[name] and not update:
            raise SystemExit(f"{path}: sha256 {sha}, expected {EXPECTED[name]}; "
                             "rerun with --update to build from this download and update EXPECTED")


def tatoeba_text(path):
    with bz2.open(path, "rt", encoding="utf-8") as f:
        for line in f:
            yield line.rstrip("\n").split("\t", 2)[2]


def is_syllable(c):
    return "가" <= c <= "힣"


def ks_syllables():
    # A syllable is in KS X 1001 when Python's euc_kr codec encodes it, the
    # same test the engine's hangul::KS_X_1001 bitmap was generated with.
    return [chr(c) for c in range(0xAC00, 0xD7A4) if len(chr(c).encode("euc_kr", "ignore")) == 2]


def nikl_counts(path):
    """Syllable counts from the 2002 survey's word, particle and ending lists.

    The lists hold lemmas, so a verb or adjective is counted as its stem
    (하다 -> 하); the ending list counts the endings separately."""
    counts = collections.Counter()
    # Frequency-ordered files: (name fragment, frequency column).
    wanted = {"단어_빈도": 4, "조사_빈도": 3, "어미_빈도": 3, "고유명사_빈도": 2}
    with zipfile.ZipFile(path) as z:
        for info in z.infolist():
            name = info.filename.encode("cp437").decode("cp949")
            col = next((c for k, c in wanted.items() if k in name), None)
            if col is None:
                continue
            for i, line in enumerate(z.read(info).decode("cp949", "replace").splitlines()):
                p = line.split("\t")
                if i == 0 or len(p) <= col:
                    continue
                try:
                    n = int(p[col].strip().replace(",", ""))
                except ValueError:
                    continue
                word = re.sub(r"\d+$", "", p[1])  # homograph number: 가01
                if "단어" in name and p[3] in ("동", "형", "보") and word.endswith("다"):
                    word = word[:-1]
                for c in word:
                    if is_syllable(c):
                        counts[c] += n
    return counts


def particles(path):
    """Particle unigram from the survey's 조사 list, one `particle\\tcost`
    line each, most frequent first. Homographs (이 주격, 이 보격) merge;
    contracted jamo particles (ㄴ, ㄹ) are dropped, a span after English
    never reads as one."""
    counts = collections.Counter()
    with zipfile.ZipFile(path) as z:
        for info in z.infolist():
            if "조사_빈도" not in info.filename.encode("cp437").decode("cp949"):
                continue
            for line in z.read(info).decode("cp949", "replace").splitlines()[1:]:
                p = line.split("\t")
                if len(p) > 3 and p[1] and all(is_syllable(c) for c in p[1]):
                    counts[p[1]] += int(p[3].strip().replace(",", ""))
    total = sum(counts.values())
    lines = [f"{w}\t{min(255, round(-math.log(k / total) * SCALE))}" for w, k in counts.most_common()]
    print(f"particles: {len(counts)} from {total} tokens")
    return ("\n".join(lines) + "\n").encode("utf-8")


def korean(cache):
    ks = ks_syllables()
    assert len(ks) == 2350
    tat = collections.Counter(c for s in tatoeba_text(cache / "kor_sentences.tsv.bz2") for c in s if is_syllable(c))
    nikl = nikl_counts(cache / "nikl_freq_2002.zip")
    # Equal-weight mixture: Tatoeba is surface text (했, 해요) but small; the
    # survey covers far more syllables but counts lemmas, not inflected forms.
    tat_n = sum(tat[c] for c in ks)
    nikl_n = sum(nikl[c] for c in ks)
    out = bytearray()
    for c in ks:
        p = 0.5 * tat[c] / tat_n + 0.5 * nikl[c] / nikl_n
        p = (1 - KO_UNIFORM) * p + KO_UNIFORM / len(ks)
        out.append(min(255, round(-math.log(p) * SCALE)))
    print(f"ko: tatoeba {tat_n} syllables, survey {nikl_n}, "
          f"seen {sum(1 for c in ks if tat[c] or nikl[c])}/2350")
    return bytes(out)


def english(cache):
    words = collections.Counter()
    for s in tatoeba_text(cache / "eng_sentences.tsv.bz2"):
        words.update(re.findall(r"[a-z]+", s.lower()))
    # A word weighs 1 + ln(count): the model judges whether an unseen word
    # looks English, and raw token counts would make it a model of "the",
    # while counting each word once leaves `no` no likelier than a typo.
    n = 27
    tri = collections.Counter()
    for w, count in words.items():
        seq = [BOUNDARY, BOUNDARY] + [ord(c) - 97 for c in w] + [BOUNDARY]
        for i in range(2, len(seq)):
            tri[(seq[i - 2], seq[i - 1], seq[i])] += 1 + math.log(count)
    bi = collections.Counter()
    uni = collections.Counter()
    for (a, b, c), k in tri.items():
        bi[(b, c)] += k
        uni[c] += k
    total = sum(uni.values())

    def witten_bell(counts, ctx_of, lower):
        # Interpolated Witten-Bell: P = (c(h,w) + T(h) P_lower(w)) / (c(h) + T(h)).
        ctx_total = collections.Counter()
        ctx_types = collections.Counter()
        for key, k in counts.items():
            ctx_total[ctx_of(key)] += k
            ctx_types[ctx_of(key)] += 1

        def p(key):
            h = ctx_of(key)
            t, c = ctx_types[h], ctx_total[h]
            if c == 0:
                return lower(key)
            return (counts[key] + t * lower(key)) / (c + t)

        return p

    def p_uni(key):
        return (uni[key[-1]] + 1) / (total + n)

    p_bi = witten_bell(bi, lambda k: k[0], p_uni)
    p_tri = witten_bell(tri, lambda k: k[:2], lambda k: p_bi(k[1:]))
    out = bytearray()
    for a in range(n):
        for b in range(n):
            for c in range(n):
                out.append(min(255, round(-math.log(p_tri((a, b, c))) * SCALE)))
    print(f"en: {len(words)} distinct words, {sum(tri.values()):.0f} weighted trigrams")
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", type=pathlib.Path, required=True)
    ap.add_argument("--update", action="store_true", help="accept downloads whose SHA-256 differs from EXPECTED")
    args = ap.parse_args()
    fetch(args.cache, args.update)
    OUT.mkdir(parents=True, exist_ok=True)
    tables = [
        ("ko_syllable.bin", korean(args.cache)),
        ("ko_particle.txt", particles(args.cache / "nikl_freq_2002.zip")),
        ("en_trigram.bin", english(args.cache)),
    ]
    for name, data in tables:
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
