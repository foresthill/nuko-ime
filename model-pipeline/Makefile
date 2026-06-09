# nuko-ime model-pipeline — libakaza モデル生成
#
# Phase 2-C 実装。各 target は実際にダウンロード + ビルドする。
# 詳細は README.md と docs/spikes/phase2-model-pipeline.md を参照。

# -------------------------------------------------------------------------
# Configuration
# -------------------------------------------------------------------------

# libakaza / akaza-data の pin
AKAZA_REV ?= 8a404281ece7ca51119127a96bdde8c153b0df61

# 上流データのバージョン (akaza 上流 Makefile に追従)
CORPUS_STATS_VERSION ?= v2026.0216.0
SUDACHI_VERSION      ?= 20260116

# 出力ディレクトリ
TOOLS_DIR ?= tools
WORK_DIR  ?= work
DATA_DIR  ?= data

# learn-corpus のプロファイル (min: 試作品、full: 本番)
PROFILE ?= min

ifeq ($(PROFILE),min)
  MAY_EPOCHS    = 2
  SHOULD_EPOCHS = 5
  MUST_EPOCHS   = 50
  DELTA         = 500
else ifeq ($(PROFILE),full)
  MAY_EPOCHS    = 10
  SHOULD_EPOCHS = 100
  MUST_EPOCHS   = 10000
  DELTA         = 2000
else
  $(error PROFILE must be 'min' or 'full', got '$(PROFILE)')
endif

AKAZA_DATA = $(TOOLS_DIR)/bin/akaza-data

# corpus-stats tarball 展開後の trie ファイル群 (extraction で位置を確認)
CORPUS_UNIGRAM_TRIE     = $(WORK_DIR)/stats-vibrato-unigram.wordcnt.trie
CORPUS_BIGRAM_TRIE      = $(WORK_DIR)/stats-vibrato-bigram.wordcnt.trie
CORPUS_SKIP_BIGRAM_TRIE = $(WORK_DIR)/stats-vibrato-skip-bigram.wordcnt.trie
VOCAB_FILE              = $(WORK_DIR)/vibrato-ipadic.vocab

UNIDIC_CSV = $(WORK_DIR)/unidic/lex_3_1.csv

# -------------------------------------------------------------------------
# High-level targets
# -------------------------------------------------------------------------

.PHONY: all help install-tools clean distclean verify

all: $(DATA_DIR)/unigram.model $(DATA_DIR)/bigram.model $(DATA_DIR)/skip_bigram.model $(DATA_DIR)/SKK-JISYO.akaza

help:
	@echo "nuko-ime model-pipeline targets:"
	@echo ""
	@echo "  make install-tools              # cargo install akaza-data into $(TOOLS_DIR)/"
	@echo "  make all PROFILE=min            # build minimum models (Phase 2-C; epochs short)"
	@echo "  make all PROFILE=full           # build full-quality models (Phase 2-E)"
	@echo "  make verify                     # smoke test: akaza-data check で疎通確認"
	@echo "  make clean                      # remove $(WORK_DIR) and $(DATA_DIR)"
	@echo "  make distclean                  # also remove $(TOOLS_DIR)"
	@echo ""
	@echo "Current PROFILE=$(PROFILE):"
	@echo "  delta=$(DELTA) may=$(MAY_EPOCHS) should=$(SHOULD_EPOCHS) must=$(MUST_EPOCHS)"

# -------------------------------------------------------------------------
# Tool installation
# -------------------------------------------------------------------------

install-tools: $(AKAZA_DATA)

$(AKAZA_DATA):
	mkdir -p $(TOOLS_DIR)
	cargo install --git https://github.com/akaza-im/akaza \
	  --rev $(AKAZA_REV) \
	  --root $(TOOLS_DIR) \
	  akaza-data

# -------------------------------------------------------------------------
# Downloads
# -------------------------------------------------------------------------

# akaza-corpus-stats tarball: Wikipedia + 青空文庫 + CC-100 + IPADIC から
# 生成された wordcnt trie + vocab ファイル群 (~270 MB)
$(WORK_DIR)/corpus-stats/_SUCCESS:
	mkdir -p $(WORK_DIR)
	gh release download $(CORPUS_STATS_VERSION) \
	  --repo akaza-im/akaza-corpus-stats \
	  --pattern 'akaza-corpus-stats-full.tar.gz' \
	  --output $(WORK_DIR)/akaza-corpus-stats.tar.gz
	tar xzf $(WORK_DIR)/akaza-corpus-stats.tar.gz -C $(WORK_DIR)/
	rm $(WORK_DIR)/akaza-corpus-stats.tar.gz
	mkdir -p $(WORK_DIR)/corpus-stats && touch $(WORK_DIR)/corpus-stats/_SUCCESS

# corpus-stats が提供するファイル群はすべて _SUCCESS マーカーに依存
$(CORPUS_UNIGRAM_TRIE) $(CORPUS_BIGRAM_TRIE) $(CORPUS_SKIP_BIGRAM_TRIE) $(VOCAB_FILE): $(WORK_DIR)/corpus-stats/_SUCCESS

# UniDic 3.1.1 CSJ (BSD-3 側を採用、NINJAL から直接取得)
# 上流 akaza は unidic-mirror release を使っていたが、当該 release が削除済のため
# NINJAL の公式アーカイブを参照する。
$(WORK_DIR)/unidic/unidic.zip:
	mkdir -p $(WORK_DIR)/unidic/
	curl -fL -o $(WORK_DIR)/unidic/unidic.zip \
	  https://clrd.ninjal.ac.jp/unidic_archive/csj/3.1.1/unidic-csj-3.1.1.zip

$(UNIDIC_CSV): $(WORK_DIR)/unidic/unidic.zip
	unzip -D -o -j $(WORK_DIR)/unidic/unidic.zip -d $(WORK_DIR)/unidic/
	touch $(UNIDIC_CSV)

# SudachiDict 各 lex CSV (固有名詞補完用、Apache-2.0)
# macOS では wget が標準でないため curl を使う
$(WORK_DIR)/sudachi/%_lex.csv:
	mkdir -p $(WORK_DIR)/sudachi
	curl -fL -o $(WORK_DIR)/sudachi/$*_lex.zip \
	  http://sudachi.s3-website-ap-northeast-1.amazonaws.com/sudachidict-raw/$(SUDACHI_VERSION)/$*_lex.zip
	unzip -o $(WORK_DIR)/sudachi/$*_lex.zip -d $(WORK_DIR)/sudachi/
	rm $(WORK_DIR)/sudachi/$*_lex.zip

# -------------------------------------------------------------------------
# SudachiDict 漢字普通名詞 → corpus 抽出
# -------------------------------------------------------------------------
#
# 上流 akaza-data の make_sudachi_dict は SudachiDict から:
#   - 名詞-固有名詞-一般
#   - 名詞-普通名詞 (表層形が**全カタカナ**のみ)
# しか拾わない設計のため、「既読 / 既得 / 危篤」のような **漢字熟語の
# 普通名詞** が SKK-JISYO.akaza に入らない (2026-06-08 実機検証で判明)。
#
# 対策: SudachiDict CSV から漢字を含む普通名詞を抽出して `--corpus` 形式
# (`surface/yomi` 1 行 1 単語) に変換し、make-dict に追加 corpus として
# 渡す。make_corpus_dict はフィルタなく全 word を dict に追加するため、
# 上流 akaza-data に手を入れずに同じ効果が得られる (= filter 緩和)。

SUDACHI_CORPUS = $(WORK_DIR)/sudachi-kanji-corpus.txt

$(SUDACHI_CORPUS): scripts/sudachi_to_corpus.py \
                   $(WORK_DIR)/sudachi/small_lex.csv \
                   $(WORK_DIR)/sudachi/core_lex.csv \
                   $(WORK_DIR)/sudachi/notcore_lex.csv
	python3 scripts/sudachi_to_corpus.py $(SUDACHI_CORPUS) \
	  $(WORK_DIR)/sudachi/small_lex.csv \
	  $(WORK_DIR)/sudachi/core_lex.csv \
	  $(WORK_DIR)/sudachi/notcore_lex.csv

# -------------------------------------------------------------------------
# SKK-JISYO.akaza generation
# -------------------------------------------------------------------------

# 上流 Makefile に倣い、make-dict は dict/SKK-JISYO.akaza + UniDic + vocab +
# SudachiDict + training-corpus から data/SKK-JISYO.akaza を出力する。
# 末尾の data/SKK-JISYO.akaza は CLI の <TXT_FILE> 位置引数 (実体としては
# SKK 辞書の出力先)。
$(DATA_DIR)/SKK-JISYO.akaza: $(AKAZA_DATA) \
                              dict/SKK-JISYO.akaza \
                              training-corpus/must.txt \
                              training-corpus/should.txt \
                              training-corpus/may.txt \
                              $(UNIDIC_CSV) \
                              $(VOCAB_FILE) \
                              $(WORK_DIR)/sudachi/small_lex.csv \
                              $(WORK_DIR)/sudachi/core_lex.csv \
                              $(WORK_DIR)/sudachi/notcore_lex.csv \
                              $(SUDACHI_CORPUS)
	mkdir -p $(DATA_DIR)
	$(AKAZA_DATA) make-dict \
	  --corpus dict/SKK-JISYO.akaza \
	  --corpus training-corpus/must.txt \
	  --corpus training-corpus/should.txt \
	  --corpus training-corpus/may.txt \
	  --corpus $(SUDACHI_CORPUS) \
	  --unidic $(UNIDIC_CSV) \
	  --vocab $(VOCAB_FILE) \
	  --sudachi-lex $(WORK_DIR)/sudachi/small_lex.csv \
	  --sudachi-lex $(WORK_DIR)/sudachi/core_lex.csv \
	  --sudachi-lex $(WORK_DIR)/sudachi/notcore_lex.csv \
	  $(DATA_DIR)/SKK-JISYO.akaza

# -------------------------------------------------------------------------
# Language model training
# -------------------------------------------------------------------------

# learn-corpus: 7 positional 引数の順序固定 (may → should → must → src_unigram
# → src_bigram → dst_unigram → dst_bigram)。skip_bigram は任意ペア。
# bigram.model のターゲットが unigram/skip_bigram もまとめて生成する。
$(DATA_DIR)/bigram.model: $(AKAZA_DATA) \
                           $(DATA_DIR)/SKK-JISYO.akaza \
                           training-corpus/must.txt \
                           training-corpus/should.txt \
                           training-corpus/may.txt \
                           $(CORPUS_UNIGRAM_TRIE) \
                           $(CORPUS_BIGRAM_TRIE) \
                           $(CORPUS_SKIP_BIGRAM_TRIE)
	mkdir -p $(DATA_DIR)
	$(AKAZA_DATA) learn-corpus \
	  --delta=$(DELTA) \
	  --may-epochs=$(MAY_EPOCHS) \
	  --should-epochs=$(SHOULD_EPOCHS) \
	  --must-epochs=$(MUST_EPOCHS) \
	  training-corpus/may.txt \
	  training-corpus/should.txt \
	  training-corpus/must.txt \
	  $(CORPUS_UNIGRAM_TRIE) \
	  $(CORPUS_BIGRAM_TRIE) \
	  $(DATA_DIR)/unigram.model \
	  $(DATA_DIR)/bigram.model \
	  --src-skip-bigram=$(CORPUS_SKIP_BIGRAM_TRIE) \
	  --dst-skip-bigram=$(DATA_DIR)/skip_bigram.model

# unigram/skip_bigram は learn-corpus が同時生成するため bigram.model に依存
$(DATA_DIR)/unigram.model $(DATA_DIR)/skip_bigram.model: $(DATA_DIR)/bigram.model

# -------------------------------------------------------------------------
# Verification (Phase 2-C 完了判定)
# -------------------------------------------------------------------------

# 疎通確認: 簡単な日本語フレーズを変換して、libakaza が生成モデルを読めて
# 何かしらの surface が返ることを確認する (品質は不問、PROFILE=min なので)。
#
# 上流の akaza-data check は libakaza::config が XDG_DATA から
# romkan/default.json を要求するため設定ファイル一式が必要。
# nuko-ime では romkan は自前なので、代わりに spikes/libakaza-with-model の
# Rust バイナリ (= LibakazaBackend を直接叩く) を疎通テストとして使う。
verify: all
	@echo "=== Phase 2-C 疎通確認 (spike: LibakazaBackend 直接呼び出し) ==="
	cargo run --release \
	  --manifest-path ../spikes/libakaza-with-model/Cargo.toml \
	  -- $(abspath $(DATA_DIR))

# -------------------------------------------------------------------------
# Maintenance
# -------------------------------------------------------------------------

clean:
	rm -rf $(WORK_DIR) $(DATA_DIR)

distclean: clean
	rm -rf $(TOOLS_DIR)
