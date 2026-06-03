# spike-4: akaza-data CLI ビルド検証

[`docs/spikes/akaza-data-cli-spike.md`](../../docs/spikes/akaza-data-cli-spike.md) を参照。

## 再現方法

```bash
mkdir -p spikes/akaza-data-cli/install
cargo install --git https://github.com/akaza-im/akaza \
  --rev 8a404281ece7ca51119127a96bdde8c153b0df61 \
  --root spikes/akaza-data-cli/install \
  akaza-data
```

成果物: `spikes/akaza-data-cli/install/bin/akaza-data` (gitignore 対象、~6.5 MB on macOS arm64)
