//! Data loader. Combines a dataset and a sampler, and provides an iterable over the given dataset.

use super::fetch::{Fetcher, MapDatasetFetcher};
use crate::{
    Dataset, Len,
    collate::{Collate, DefaultCollate},
    sampler::{BatchIterator, BatchSampler, Sampler, SequentialSampler},
};

mod builder;
use builder::Builder;

/// Data loader. Combines a dataset and a sampler, and provides an iterable over the given dataset.
///
///
/// ```rust
/// use ai_dataloader::indexable::DataLoader;
///
/// let loader = DataLoader::builder(vec![(0, "hola"), (1, "hello"), (2, "hallo"), (3, "bonjour")]).batch_size(2).shuffle().build();
///
/// for (label, text) in &loader {
///     println!("Label {label:?}");
///     println!("Text {text:?}");
/// }
/// ```
///
#[derive(Debug, Clone, PartialEq, PartialOrd, Hash, Eq, Ord)]
pub struct DataLoader<D, S = SequentialSampler, C = DefaultCollate> {
    /// Dataset from which to load the data.
    dataset: D,
    /// Return a batch of indices at a time.
    batch_sampler: BatchSampler<S>,
    /// Collate function.
    collate_fn: C,
}

impl<D> DataLoader<D, SequentialSampler, DefaultCollate>
where
    D: Dataset,
    DefaultCollate: Collate<D::Sample>,
{
    /// Helper to return a [`DataLoader`] builder.
    pub fn builder(dataset: D) -> Builder<D, SequentialSampler, DefaultCollate> {
        Builder::new(dataset)
    }
}

impl<D, S, C> DataLoader<D, S, C>
where
    D: Dataset + Sync,
    S: Sampler,
    C: Collate<D::Sample>,
    D::Sample: Send,
{
    /// Return not owning iterator over the dataloader.
    pub fn iter(&self) -> SingleProcessDataLoaderIter<'_, D, S, C> {
        SingleProcessDataLoaderIter::new(self)
    }
}

impl<D, S, C> Len for DataLoader<D, S, C>
where
    D: Dataset,
    S: Sampler,
    C: Collate<D::Sample>,
{
    /// Return the number of batch that contain the dataloader.
    fn len(&self) -> usize {
        self.batch_sampler.len()
    }
}

/// Iterate over the dataloader with a single thread.
#[derive(Debug)]
pub struct SingleProcessDataLoaderIter<'dataset, D, S = SequentialSampler, C = DefaultCollate>
where
    D: Dataset + Sync,
    S: Sampler,
    C: Collate<D::Sample>,
{
    /// The batch iterator of this iterator.
    sampler_iter: BatchIterator<S::IntoIter>,
    /// Number of sample yielded.
    num_yielded: u64,
    /// Used to fetch the data from the dataset.
    data_fetcher: MapDatasetFetcher<'dataset, D, C>,
}

impl<D, S, C> SingleProcessDataLoaderIter<'_, D, S, C>
where
    D: Dataset + Sync,
    S: Sampler,
    C: Collate<D::Sample>,
    D::Sample: Send,
{
    fn new(loader: &DataLoader<D, S, C>) -> SingleProcessDataLoaderIter<'_, D, S, C> {
        SingleProcessDataLoaderIter {
            sampler_iter: loader.batch_sampler.iter(),
            num_yielded: 0,
            data_fetcher: MapDatasetFetcher {
                dataset: &loader.dataset,
                collate_fn: &loader.collate_fn,
            },
        }
    }
    fn next_index(&mut self) -> Option<Vec<usize>> {
        self.sampler_iter.next()
    }
    fn next_data(&mut self) -> Option<C::Output> {
        let index = self.next_index();
        if let Some(index) = index {
            let data = self.data_fetcher.fetch(index);
            return Some(data);
        }
        None
    }
}

impl<D, S, C> Iterator for SingleProcessDataLoaderIter<'_, D, S, C>
where
    D: Dataset + Sync,
    S: Sampler,
    C: Collate<D::Sample>,
    D::Sample: Send,
{
    type Item = C::Output;
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.next_data();

        if let Some(data) = data {
            self.num_yielded += 1;
            return Some(data);
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, upper) = self.sampler_iter.size_hint();
        (lower, upper)
    }
}

impl<'dataset, D, S, C> IntoIterator for &'dataset DataLoader<D, S, C>
where
    D: Dataset + Sync,
    S: Sampler,
    C: Collate<D::Sample>,
    D::Sample: Send,
{
    type Item = C::Output;
    type IntoIter = SingleProcessDataLoaderIter<'dataset, D, S, C>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<D, S, C> ExactSizeIterator for SingleProcessDataLoaderIter<'_, D, S, C>
where
    D: Dataset + Sync,
    S: Sampler,
    S::IntoIter: ExactSizeIterator,
    C: Collate<D::Sample>,
    D::Sample: Send,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Len;
    use crate::NdarrayDataset;
    use crate::collate::NoOpCollate;
    use crate::sampler::RandomSampler;
    use crate::sampler::SequentialSampler;
    use ndarray::{Array, Array1, Array4, Axis, Ix1, Ix4, Slice, arr0, array};
    use ndarray_rand::RandomExt;
    use ndarray_rand::rand_distr::{Normal, Uniform};
    use std::collections::HashMap;

    #[test]
    fn len() {
        let dataset = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let dataloader = DataLoader::builder(dataset)
            .batch_size(2)
            .drop_last()
            .build();
        assert_eq!(dataloader.len(), dataloader.batch_sampler.len());
        assert_eq!(dataloader.len(), 5);
        let mut iter = dataloader.iter();
        assert_eq!(iter.len(), 5);
        iter.next();
        assert_eq!(iter.len(), 4);
    }

    #[test]
    fn one_dimension_basic() {
        let dataset = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let dataloader = DataLoader::builder(dataset).batch_size(2).build();

        let mut iter = dataloader.iter();
        assert_eq!(iter.next(), Some(array![1, 2]));
        assert_eq!(iter.next(), Some(array![3, 4]));
        assert_eq!(iter.next(), Some(array![5, 6]));
        assert_eq!(iter.next(), Some(array![7, 8]));
        assert_eq!(iter.next(), Some(array![9, 10]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn two_iteration() {
        let dataset = vec![1, 2, 3, 4];
        let dataloader = DataLoader::builder(dataset).batch_size(2).build();

        let mut iter = dataloader.iter();
        assert_eq!(iter.next(), Some(array![1, 2]));
        assert_eq!(iter.next(), Some(array![3, 4]));
        assert_eq!(iter.next(), None);
        let mut iter = dataloader.iter();
        assert_eq!(iter.next(), Some(array![1, 2]));
        assert_eq!(iter.next(), Some(array![3, 4]));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn one_dimension_basic_string() {
        let dataset = vec![String::from("a"), String::from("b")];
        let dataloader = DataLoader::builder(dataset).build();

        let mut iter = dataloader.iter();
        assert_eq!(iter.next(), Some(vec![String::from("a")]));
        assert_eq!(iter.next(), Some(vec![String::from("b")]));
        assert_eq!(iter.next(), None);
    }
    #[test]
    fn collate() {
        let dataset = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let dataloader = DataLoader::builder(dataset)
            .batch_size(2)
            .collate_fn(NoOpCollate)
            .build();

        let mut iter = dataloader.iter();

        assert_eq!(iter.next(), Some(vec![1, 2]));
        assert_eq!(iter.next(), Some(vec![3, 4]));
        assert_eq!(iter.next(), Some(vec![5, 6]));
        assert_eq!(iter.next(), Some(vec![7, 8]));
        assert_eq!(iter.next(), Some(vec![9, 10]));
        assert_eq!(iter.next(), None);
    }
    struct TestDataLoader<S: Sampler> {
        loader: DataLoader<NdarrayDataset<f64, f64, Ix4, Ix1>, S>,
        data: Array4<f64>,
        labels: Array1<f64>,
        dataset: NdarrayDataset<f64, f64, Ix4, Ix1>,
    }
    enum TestDataLoaderData {
        Sequential(TestDataLoader<SequentialSampler>),
        Random(TestDataLoader<RandomSampler>),
    }
    fn get_loader_with_dummy_data(batch_size: usize, shuffle: bool) -> TestDataLoaderData {
        // We use a normal distribution for the random numbers
        let normal: Normal<f64> = Normal::new(0.0, 1.0).unwrap();
        // We create a 4-dimensional array populated with random value
        let data = Array::random((100, 2, 3, 5), normal);
        // We create a 1-dimensional array populated with random value
        let labels = Array::random(100, Uniform::<f64>::new(0., 50.));
        // Basic Test dataset
        let dataset = NdarrayDataset {
            ndarrays: (data.clone(), labels.clone()),
        };

        if shuffle {
            let loader = DataLoader::builder(dataset.clone())
                .batch_size(batch_size)
                .shuffle()
                .build();

            TestDataLoaderData::Random(TestDataLoader {
                loader,
                data,
                labels,
                dataset,
            })
        } else {
            let loader = DataLoader::builder(dataset.clone())
                .batch_size(batch_size)
                .build();

            TestDataLoaderData::Sequential(TestDataLoader {
                loader,
                data,
                labels,
                dataset,
            })
        }
    }

    #[test]
    fn sequential_non_batch() {
        let batch_size = 1;
        let test_dataloader_data = tests::get_loader_with_dummy_data(batch_size, false);
        let test_data;
        if let TestDataLoaderData::Sequential(test_dataloader_data) = test_dataloader_data {
            test_data = test_dataloader_data;
        } else {
            panic!("Expected a sequential loader")
        }
        let mut current_idx = 0;

        for (idx, (sample, target)) in test_data.loader.iter().enumerate() {
            assert_eq!(
                sample,
                test_data
                    .data
                    .slice_axis(Axis(0), Slice::from(idx..idx + batch_size))
            );
            assert_eq!(
                target,
                test_data
                    .labels
                    .slice_axis(Axis(0), Slice::from(idx..idx + batch_size))
            );
            current_idx = idx;
        }
        assert_eq!(current_idx, test_data.dataset.len() - 1);
    }

    #[test]
    fn sequential_batch() {
        let batch_size = 2;
        let test_dataloader_data = tests::get_loader_with_dummy_data(2, false);
        let test_data;
        if let TestDataLoaderData::Sequential(test_dataloader_data) = test_dataloader_data {
            test_data = test_dataloader_data;
        } else {
            panic!("Expected a sequential loader")
        }

        let mut current_i = 0;

        for (i, (sample, target)) in test_data.loader.iter().enumerate() {
            let idx = i * batch_size;
            assert_eq!(
                sample,
                test_data
                    .data
                    .slice_axis(Axis(0), Slice::from(idx..idx + batch_size))
            );
            assert_eq!(
                target,
                test_data
                    .labels
                    .slice_axis(Axis(0), Slice::from(idx..idx + batch_size))
            );
            current_i = i;
        }
        assert_eq!(current_i, (test_data.dataset.len() - 1) / batch_size);
    }

    #[test]
    fn shuffle_non_batch() {
        let test_dataloader_data = tests::get_loader_with_dummy_data(1, true);
        let test_data;
        if let TestDataLoaderData::Random(test_dataloader_data) = test_dataloader_data {
            test_data = test_dataloader_data;
        } else {
            panic!("Expected a random loader")
        }
        // 2 maps to keep track on what we have iterated.
        let mut found_data: HashMap<_, _> = (0..test_data.data.len())
            .zip(vec![0; test_data.data.len()])
            .collect();
        let mut found_labels: HashMap<_, _> = (0..test_data.labels.len())
            .zip(vec![0; test_data.labels.len()])
            .collect();
        let mut current_i = 0;
        for (i, (sample, target)) in test_data.loader.iter().enumerate() {
            current_i = i;
            let mut current_data_point_idx = 0;
            // We iterate over the original data, finding the data corresponding to the one the dataloader just yield us
            for (data_point_idx, data_point) in test_data.data.outer_iter().enumerate() {
                current_data_point_idx = data_point_idx;
                // We need to take the inner of the sample (It's not automatically done like in python)
                if data_point == sample.index_axis(Axis(0), 0) {
                    assert_eq!(found_data[&data_point_idx], 0);
                    *found_data.get_mut(&data_point_idx).unwrap() += 1;
                    break;
                }
            }

            assert_eq!(
                arr0(target[0]),
                test_data.labels.index_axis(Axis(0), current_data_point_idx)
            );
            *found_labels.get_mut(&current_data_point_idx).unwrap() += 1;
            assert_eq!(found_data.values().sum::<usize>(), i + 1);
            assert_eq!(found_labels.values().sum::<usize>(), i + 1);
        }
        assert_eq!(current_i, test_data.dataset.len() - 1);
    }

    #[test]
    fn shuffle_batch() {
        let batch_size = 2;
        let test_dataloader_data = tests::get_loader_with_dummy_data(batch_size, true);
        let test_data;
        if let TestDataLoaderData::Random(test_dataloader_data) = test_dataloader_data {
            test_data = test_dataloader_data;
        } else {
            panic!("Expected a random loader")
        }
        let mut found_data: HashMap<_, _> = (0..test_data.data.len())
            .zip(vec![0; test_data.data.len()])
            .collect();
        let mut found_labels: HashMap<_, _> = (0..test_data.labels.len())
            .zip(vec![0; test_data.labels.len()])
            .collect();
        let mut current_i = 0;
        for (i, (batch_samples, batch_targets)) in test_data.loader.iter().enumerate() {
            current_i = i;
            for (sample, target) in batch_samples.outer_iter().zip(batch_targets) {
                let mut current_data_point_idx = 0;
                for (data_point_idx, data_point) in test_data.data.outer_iter().enumerate() {
                    current_data_point_idx = data_point_idx;
                    if data_point == sample {
                        assert_eq!(found_data[&data_point_idx], 0);
                        *found_data.get_mut(&data_point_idx).unwrap() += 1;
                        break;
                    }
                }
                assert_eq!(
                    arr0(target),
                    test_data.labels.index_axis(Axis(0), current_data_point_idx)
                );
                *found_labels.get_mut(&current_data_point_idx).unwrap() += 1;
            }
            assert_eq!(found_data.values().sum::<usize>(), (i + 1) * batch_size);
            assert_eq!(found_labels.values().sum::<usize>(), (i + 1) * batch_size);
        }
        assert_eq!(current_i, (test_data.dataset.len() - 1) / batch_size);
    }

    #[test]
    fn vec_of_token() {
        let dataset = vec![
            (0, vec![1, 23, 4, 0]),
            (1, vec![4, 0, 0, 0]),
            (1, vec![8, 23, 12, 3]),
            (0, vec![2, 45, 4, 0]),
        ];

        let loader = DataLoader::builder(dataset).batch_size(2).build();

        let mut iter = loader.iter();

        assert_eq!(
            iter.next(),
            Some((
                array![0, 1],
                vec![array![1, 4], array![23, 0], array![4, 0], array![0, 0]]
            ))
        );
    }
}
