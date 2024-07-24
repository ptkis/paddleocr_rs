use std::{path::Path, borrow::Cow};
use image::{DynamicImage, GenericImageView};
use ndarray::{Array, ArrayBase, Dim, OwnedRepr, Axis, s};
use ort::{inputs, Session};

use crate::{error::PaddleOcrResult, PaddleOcrError};

pub struct Rec {
    model: Session,
    keys: Vec<char>,
    min_score: f32
}

impl Rec {
    const MIN_SCORE_DEFAULT: f32 = 0.8;

    pub fn new(model: Session, keys: Vec<char>) -> Self {
        Self { model, keys, min_score: Self::MIN_SCORE_DEFAULT }
    }

    pub fn from_file(model_path: impl AsRef<Path>, keys_path: impl AsRef<Path>) -> PaddleOcrResult<Self> {
        let model = ort::Session::builder()?.commit_from_file(model_path)?;
        let keys = " ".chars()
            .chain(std::fs::read_to_string(keys_path)?
            .chars()
            .filter(|x| *x != '\n'))
            .chain(" ".chars())
            .collect();
        Ok(Self { model, keys, min_score: Self::MIN_SCORE_DEFAULT })
    }

    pub fn from_memory(model_content: &[u8], keys_content: &[u8]) -> PaddleOcrResult<Self> {
        let model = ort::Session::builder()?.commit_from_memory(model_content)?;
        let keys = " ".chars()
            .chain(String::from_utf8_lossy(keys_content)
            .chars()
            .filter(|x| *x != '\n'))
            .chain(" ".chars())
            .collect();
        Ok(Self { model, keys, min_score: Self::MIN_SCORE_DEFAULT })
    }

    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = min_score;
        self
    }

    pub fn predict_char_score(&self, img: &DynamicImage) -> PaddleOcrResult<Vec<(char, f32)>> {
        let input = Self::preprocess(img)?;
        let output = self.run_model(&input)?;
        Ok(output)
    }

    pub fn predict_str(&self, img: &DynamicImage) -> PaddleOcrResult<String> {
        let ret = self.predict_char_score(img)?;
        Ok(ret.into_iter().map(|x| x.0).collect())
    }

    fn preprocess(img: &DynamicImage) -> PaddleOcrResult<ArrayBase<OwnedRepr<f32>, Dim<[usize; 4]>>> {
        let (w, h) = img.dimensions();
        let img = if h <= 48 {
            Cow::Borrowed(img)
        } else {
            Cow::Owned(img.resize_exact(w, 48, image::imageops::FilterType::CatmullRom))
        };    
        let mut input = Array::zeros((1, 3, 48, w as usize));
        for pixel in img.pixels() {
            let x = pixel.0 as _;
            let y = pixel.1 as _;
            let [r, g, b, _] = pixel.2 .0;
            input[[0, 0, y, x]] = (((r as f32) / 255.) - 0.5) / 0.5;
            input[[0, 1, y, x]] = (((g as f32) / 255.) - 0.5) / 0.5;
            input[[0, 2, y, x]] = (((b as f32) / 255.) - 0.5) / 0.5;
        }
        Ok(input)
    }

    fn run_model(&self, input: &ArrayBase<OwnedRepr<f32>, Dim<[usize; 4]>>) -> PaddleOcrResult<Vec<(char, f32)>>{
        let outputs = self.model.run(inputs!["x" => input.view()]?)?;
        let output = outputs.iter().next().ok_or(PaddleOcrError::custom("no output"))?.1;
        let output = output.try_extract_tensor::<f32>()?;
        let output = output.view();
        let output = output.slice(s![0, .., ..]);
        let output = output.axis_iter(Axis(0))
            .filter_map(|x| {
                x.iter().copied().enumerate().max_by(|(_, x), (_, y)|{
                    x.total_cmp(y)
                })
            })
            .filter(|(index, score)|{
                *index != 0 && *score > self.min_score
            })
            .filter_map(|(index, score)|{
                self.keys.get(index).map(|x| (*x, score))
            })
            .collect();
        Ok(output)
    }
}
