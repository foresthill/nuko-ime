# nuko-ime model-pipeline — libakaza モデル生成
#
# 現状: Phase 2-B (scaffold)。各 target は骨格のみで、実体は Phase 2-C 以降で実装。
# 詳細は README.md と docs/spikes/phase2-model-pipeline.md を参照。

# -------------------------------------------------------------------------
# Configuration
# -------------------------------------------------------------------------

# libakaza / akaza-data の pin (libakaza-api-survey で確定した rev)
AKAZA_REV ?= 8a404281ece7ca51119127a96bdde8c153b0df61

# corpus-stats / UniDic / SudachiDict のバージョン (上流 Makefile に追従)
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

# -------------------------------------------------------------------------
# High-level targets
# -------------------------------------------------------------------------

.PHONY: all help install-tools clean distclean

all: $(DATA_DIR)/unigram.model $(DATA_DIR)/bigram.model $(DATA_DIR)/skip_bigram.model $(DATA_DIR)/SKK-JISYO.akaza

help:
	@echo "nuko-ime model-pipeline targets:"
	@echo ""
	@echo "  make install-tools              # cargo install akaza-data into $(TOOLS_DIR)/"
	@echo "  make all PROFILE=min            # build minimum models (Phase 2-C; epochs short)"
	@echo "  make all PROFILE=full           # build full-quality models (Phase 2-E)"
	@echo "  make clean                      # remove $(WORK_DIR) and $(DATA_DIR)"
	@echo "  make distclean                  # also remove $(TOOLS_DIR)"
	@echo ""
	@echo "Current PROFILE=$(PROFILE):"
	@echo "  delta=$(DELTA) may=$(MAY_EPOCHS) should=$(SHOULD_EPOCHS) must=$(MUST_EPOCHS)"
	@echo ""
	@echo "NOTE: Phase 2-B scaffold — all build targets are currently placeholders."
	@echo "      Concrete implementation lands in Phase 2-C."

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
# Downloads (Phase 2-C で実装)
# -------------------------------------------------------------------------

$(WORK_DIR)/corpus-stats/_SUCCESS:
	@echo "TODO Phase 2-C: gh release download $(CORPUS_STATS_VERSION) from akaza-im/akaza-corpus-stats"
	@false

$(WORK_DIR)/unidic/lex_3_1.csv:
	@echo "TODO Phase 2-C: download UniDic 3.1.1 (BSD-3 side)"
	@false

$(WORK_DIR)/sudachi/small_lex.csv:
	@echo "TODO Phase 2-C: download SudachiDict small_lex.csv ($(SUDACHI_VERSION))"
	@false

$(WORK_DIR)/vibrato-ipadic.vocab:
	@echo "TODO Phase 2-C: download IPADIC via Vibrato"
	@false

# -------------------------------------------------------------------------
# SKK-JISYO.akaza generation (Phase 2-C で実装)
# -------------------------------------------------------------------------

$(DATA_DIR)/SKK-JISYO.akaza: $(AKAZA_DATA) \
                              seed/SKK-JISYO.akaza \
                              $(WORK_DIR)/unidic/lex_3_1.csv \
                              $(WORK_DIR)/vibrato-ipadic.vocab \
                              $(WORK_DIR)/sudachi/small_lex.csv
	@mkdir -p $(DATA_DIR)
	@echo "TODO Phase 2-C: akaza-data make-dict --unidic ... --vocab ... --sudachi-lex ... \\"
	@echo "  --corpus seed/SKK-JISYO.akaza  $(DATA_DIR)/SKK-JISYO.akaza.txt"
	@echo "  (skeleton; actual invocation in Phase 2-C)"
	@false

# -------------------------------------------------------------------------
# Language model training (Phase 2-C で実装)
# -------------------------------------------------------------------------

$(DATA_DIR)/bigram.model $(DATA_DIR)/unigram.model $(DATA_DIR)/skip_bigram.model: \
    $(AKAZA_DATA) \
    $(DATA_DIR)/SKK-JISYO.akaza \
    $(WORK_DIR)/corpus-stats/_SUCCESS
	@mkdir -p $(DATA_DIR)
	@echo "TODO Phase 2-C: akaza-data learn-corpus --delta $(DELTA) \\"
	@echo "  --may-epochs $(MAY_EPOCHS) --should-epochs $(SHOULD_EPOCHS) --must-epochs $(MUST_EPOCHS) \\"
	@echo "  <may.txt> <should.txt> <must.txt> \\"
	@echo "  $(WORK_DIR)/corpus-stats/stats-vibrato-unigram.wordcnt.trie \\"
	@echo "  $(WORK_DIR)/corpus-stats/stats-vibrato-bigram.wordcnt.trie \\"
	@echo "  $(DATA_DIR)/unigram.model $(DATA_DIR)/bigram.model \\"
	@echo "  --src-skip-bigram $(WORK_DIR)/corpus-stats/stats-vibrato-skip-bigram.wordcnt.trie \\"
	@echo "  --dst-skip-bigram $(DATA_DIR)/skip_bigram.model"
	@false

# -------------------------------------------------------------------------
# Maintenance
# -------------------------------------------------------------------------

clean:
	rm -rf $(WORK_DIR) $(DATA_DIR)

distclean: clean
	rm -rf $(TOOLS_DIR)
