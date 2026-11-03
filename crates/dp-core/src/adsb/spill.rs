//! Bounded-memory external sort: rows are spilled to one Parquet file per bucket, then each bucket is
//! loaded alone, sorted and rewritten. Bucket keys are chosen so that bucket order = global order.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use arrow_array::{RecordBatch, RecordBatchReader, UInt32Array};
use arrow_schema::SchemaRef;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::properties::WriterProperties;

use super::schema::writer_props;

pub struct BucketSpill<R> {
    dir: PathBuf,
    name: &'static str,
    schema: SchemaRef,
    to_batch: fn(&[R]) -> RecordBatch,
    flush_rows: usize,
    buffers: BTreeMap<i64, Vec<R>>,
    writers: BTreeMap<i64, (PathBuf, ArrowWriter<File>)>,
    props: WriterProperties,
}

impl<R> BucketSpill<R> {
    pub fn new(dir: &Path, name: &'static str, schema: SchemaRef, to_batch: fn(&[R]) -> RecordBatch) -> Self {
        Self {
            dir: dir.to_path_buf(),
            name,
            schema,
            to_batch,
            flush_rows: 8192,
            buffers: BTreeMap::new(),
            writers: BTreeMap::new(),
            props: writer_props(1, 65_536, Vec::new()),
        }
    }

    pub fn push(&mut self, key: i64, row: R) -> anyhow::Result<()> {
        let buf = self.buffers.entry(key).or_default();
        buf.push(row);
        if buf.len() >= self.flush_rows {
            let rows = std::mem::take(buf);
            self.write(key, &rows)?;
        }
        Ok(())
    }

    fn write(&mut self, key: i64, rows: &[R]) -> anyhow::Result<()> {
        if !self.writers.contains_key(&key) {
            let path = self.dir.join(format!("{}-{key}.spill.parquet", self.name));
            let w = ArrowWriter::try_new(File::create(&path)?, self.schema.clone(), Some(self.props.clone()))?;
            self.writers.insert(key, (path, w));
        }
        let (_, w) = self.writers.get_mut(&key).expect("inserted above");
        w.write(&(self.to_batch)(rows))?;
        Ok(())
    }

    /// Flush and close every bucket; returns spill files in ascending key order.
    pub fn finish(mut self) -> anyhow::Result<Vec<(i64, PathBuf)>> {
        for (key, rows) in std::mem::take(&mut self.buffers) {
            if !rows.is_empty() {
                self.write(key, &rows)?;
            }
        }
        let mut out = Vec::with_capacity(self.writers.len());
        for (key, (path, w)) in self.writers {
            w.close()?;
            out.push((key, path));
        }
        Ok(out)
    }
}

pub fn read_all(path: &Path) -> anyhow::Result<RecordBatch> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?.with_batch_size(65_536).build()?;
    let schema = reader.schema();
    let batches = reader.collect::<Result<Vec<_>, _>>()?;
    Ok(arrow_select::concat::concat_batches(&schema, &batches)?)
}

/// Load one spill file, reorder it by `order` (a permutation from the batch), write `out`, delete the spill.
pub fn sort_spill(
    spill: &Path,
    out: &Path,
    props: WriterProperties,
    order: impl Fn(&RecordBatch) -> Vec<u32>,
) -> anyhow::Result<usize> {
    let batch = read_all(spill)?;
    let idx = UInt32Array::from(order(&batch));
    let sorted = arrow_select::take::take_record_batch(&batch, &idx)?;
    drop(batch);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut w = ArrowWriter::try_new(File::create(out)?, sorted.schema(), Some(props))?;
    w.write(&sorted)?;
    w.close()?;
    std::fs::remove_file(spill)?;
    Ok(sorted.num_rows())
}

/// Stable permutation sorting rows by `key(i)`.
pub fn permutation<K: PartialOrd>(n: usize, key: impl Fn(usize) -> K) -> Vec<u32> {
    let mut idx: Vec<u32> = (0..n as u32).collect();
    idx.sort_by(|&a, &b| key(a as usize).partial_cmp(&key(b as usize)).unwrap_or(std::cmp::Ordering::Equal));
    idx
}
