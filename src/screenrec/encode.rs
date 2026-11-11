//! The MP4 writer: a Media Foundation sink writer with an H.264 video stream
//! fed NV12 textures straight from the GPU (a hardware encoder when one
//! takes them, the Windows software encoder otherwise) and an optional AAC
//! stream fed 16-bit PCM.

use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_RENDER_TARGET, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize, IMF2DBuffer, IMFAttributes,
    IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSinkWriter,
    IMFSinkWriterEx, IMFTransform, IMFVideoSampleAllocatorEx, MF_E_SAMPLEALLOCATOR_EMPTY,
    MF_MT_AAC_PAYLOAD_TYPE, MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SA_D3D11_BINDFLAGS,
    MF_SINK_WRITER_D3D_MANAGER, MF_TRANSCODE_CONTAINERTYPE, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFMediaType_Audio, MFMediaType_Video, MFNominalRange_16_235, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_HARDWARE_URL_Attribute, MFT_FRIENDLY_NAME_Attribute, MFTranscodeContainerType_MPEG4,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709,
    MFVideoTransFunc_709, MFVideoTransferMatrix_BT709,
    eAVEncCommonRateControlMode_UnconstrainedVBR, eAVEncH264VProfile_High,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, PWSTR};

use super::Error;
use super::audio::{SAMPLE_RATE, frames_hns};
use crate::mf::{Mf, created};

/// The video stream's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSpec {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
}

/// Which H.264 encoder Media Foundation picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderInfo {
    pub hardware: bool,
    /// The encoder's friendly name when it has one.
    pub name: String,
}

/// The AAC bitrate (bytes per second, one of the values the Windows AAC
/// encoder takes): 128 kbit/s stereo, 96 kbit/s mono.
pub fn aac_bytes_per_second(channels: u16) -> u32 {
    if channels >= 2 { 16_000 } else { 12_000 }
}

/// Two 32-bit halves in one attribute (`MFSetAttributeSize`/`Ratio`).
fn pack(hi: u32, lo: u32) -> u64 {
    (hi as u64) << 32 | lo as u64
}

fn set32(a: &IMFAttributes, key: GUID, v: u32) -> windows::core::Result<()> {
    unsafe { a.SetUINT32(&key, v) }
}

fn set64(a: &IMFAttributes, key: GUID, v: u64) -> windows::core::Result<()> {
    unsafe { a.SetUINT64(&key, v) }
}

fn set_guid(a: &IMFAttributes, key: GUID, v: GUID) -> windows::core::Result<()> {
    unsafe { a.SetGUID(&key, &v) }
}

fn new_attributes(mf: &Mf, size: u32) -> windows::core::Result<IMFAttributes> {
    created(|p| unsafe { (mf.create_attributes)(p, size) })
}

fn video_type(mf: &Mf, spec: &VideoSpec, subtype: GUID) -> windows::core::Result<IMFMediaType> {
    let t: IMFMediaType = created(|p| unsafe { (mf.create_media_type)(p) })?;
    set_guid(&t, MF_MT_MAJOR_TYPE, MFMediaType_Video)?;
    set_guid(&t, MF_MT_SUBTYPE, subtype)?;
    set32(
        &t,
        MF_MT_INTERLACE_MODE,
        MFVideoInterlace_Progressive.0 as u32,
    )?;
    set64(&t, MF_MT_FRAME_SIZE, pack(spec.width, spec.height))?;
    set64(&t, MF_MT_FRAME_RATE, pack(spec.fps, 1))?;
    set64(&t, MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
    set32(&t, MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
    set32(
        &t,
        MF_MT_VIDEO_NOMINAL_RANGE,
        MFNominalRange_16_235.0 as u32,
    )?;
    set32(&t, MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
    set32(&t, MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
    Ok(t)
}

fn audio_type(mf: &Mf, channels: u16, aac: bool) -> windows::core::Result<IMFMediaType> {
    let t: IMFMediaType = created(|p| unsafe { (mf.create_media_type)(p) })?;
    let ch = channels as u32;
    set_guid(&t, MF_MT_MAJOR_TYPE, MFMediaType_Audio)?;
    set32(&t, MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
    set32(&t, MF_MT_AUDIO_SAMPLES_PER_SECOND, SAMPLE_RATE)?;
    set32(&t, MF_MT_AUDIO_NUM_CHANNELS, ch)?;
    if aac {
        set_guid(&t, MF_MT_SUBTYPE, MFAudioFormat_AAC)?;
        set32(
            &t,
            MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
            aac_bytes_per_second(channels),
        )?;
        set32(&t, MF_MT_AAC_PAYLOAD_TYPE, 0)?;
    } else {
        set_guid(&t, MF_MT_SUBTYPE, MFAudioFormat_PCM)?;
        set32(&t, MF_MT_AUDIO_BLOCK_ALIGNMENT, 2 * ch)?;
        set32(&t, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, SAMPLE_RATE * 2 * ch)?;
        set32(&t, MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
    }
    Ok(t)
}

const GENERIC_HARDWARE: &str = "hardware H.264 encoder";

/// The encoder the sink writer put on the video stream.
fn encoder_info(writer: &IMFSinkWriter, stream: u32) -> EncoderInfo {
    let mut info = EncoderInfo {
        hardware: false,
        name: String::new(),
    };
    let Ok(ex) = writer.cast::<IMFSinkWriterEx>() else {
        return info;
    };
    for i in 0..8 {
        let mut category = GUID::zeroed();
        let mut mft: Option<IMFTransform> = None;
        if unsafe { ex.GetTransformForStream(stream, i, Some(&mut category), &mut mft) }.is_err() {
            break;
        }
        if category != MFT_CATEGORY_VIDEO_ENCODER {
            continue;
        }
        let Some(attrs) = mft.and_then(|m| unsafe { m.GetAttributes() }.ok()) else {
            break;
        };
        info.hardware = unsafe { attrs.GetStringLength(&MFT_ENUM_HARDWARE_URL_Attribute) }.is_ok();
        let mut name = PWSTR::null();
        let mut len = 0u32;
        if unsafe { attrs.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len) }
            .is_ok()
        {
            info.name = unsafe { name.to_string() }.unwrap_or_default();
            unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(name.0.cast())) };
        }
        break;
    }
    if info.name.is_empty() {
        info.name = if info.hardware {
            GENERIC_HARDWARE.into()
        } else {
            "Microsoft H.264 software encoder".into()
        };
    }
    info
}

/// An open MP4 being written.
pub struct Encoder {
    mf: &'static Mf,
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<(u32, u16)>,
    allocator: IMFVideoSampleAllocatorEx,
    frame_hns: i64,
    info: EncoderInfo,
}

impl Encoder {
    /// Opens `path` for writing. Tries a hardware encoder first when
    /// `prefer_hardware`, then the software one.
    pub fn create(
        mf: &'static Mf,
        path: &Path,
        manager: &IMFDXGIDeviceManager,
        spec: &VideoSpec,
        audio_channels: Option<u16>,
        prefer_hardware: bool,
    ) -> Result<Encoder, Error> {
        let tries: &[bool] = if prefer_hardware {
            &[true, false]
        } else {
            &[false]
        };
        let mut last = Error::NoEncoder(String::new());
        for &hardware in tries {
            match Self::open(mf, path, manager, spec, audio_channels, hardware) {
                Ok(e) => return Ok(e),
                Err(e) => {
                    let _ = std::fs::remove_file(path);
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn open(
        mf: &'static Mf,
        path: &Path,
        manager: &IMFDXGIDeviceManager,
        spec: &VideoSpec,
        audio_channels: Option<u16>,
        hardware: bool,
    ) -> Result<Encoder, Error> {
        let fail = |step: &'static str| move |e: windows::core::Error| Error::win(step, &e);
        let no_encoder = |e: windows::core::Error| {
            Error::NoEncoder(format!(
                "{}x{} @ {} fps: {e}",
                spec.width, spec.height, spec.fps
            ))
        };
        let attrs = new_attributes(mf, 4).map_err(fail("MFCreateAttributes"))?;
        set32(
            &attrs,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
            hardware as u32,
        )
        .map_err(fail("attributes"))?;
        set_guid(
            &attrs,
            MF_TRANSCODE_CONTAINERTYPE,
            MFTranscodeContainerType_MPEG4,
        )
        .map_err(fail("attributes"))?;
        unsafe { attrs.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, manager) }
            .map_err(fail("attributes"))?;
        let url = HSTRING::from(path.as_os_str());
        let writer: IMFSinkWriter = created(|p| unsafe {
            (mf.create_sink_writer)(PCWSTR(url.as_ptr()), null_mut(), attrs.as_raw(), p)
        })
        .map_err(fail("MFCreateSinkWriterFromURL"))?;

        // Video: H.264 out, NV12 in.
        let out = video_type(mf, spec, MFVideoFormat_H264).map_err(fail("video type"))?;
        set32(&out, MF_MT_AVG_BITRATE, spec.bitrate).map_err(fail("video type"))?;
        set32(&out, MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)
            .map_err(fail("video type"))?;
        let video = unsafe { writer.AddStream(&out) }.map_err(no_encoder)?;
        let input = video_type(mf, spec, MFVideoFormat_NV12).map_err(fail("video type"))?;
        let params = new_attributes(mf, 2).map_err(fail("MFCreateAttributes"))?;
        set32(
            &params,
            CODECAPI_AVEncCommonRateControlMode,
            eAVEncCommonRateControlMode_UnconstrainedVBR.0 as u32,
        )
        .map_err(fail("encoder settings"))?;
        // A key frame every 2 s: quick seeking, small files.
        set32(&params, CODECAPI_AVEncMPVGOPSize, spec.fps * 2).map_err(fail("encoder settings"))?;
        unsafe { writer.SetInputMediaType(video, &input, &params) }.map_err(no_encoder)?;

        // Audio: AAC out, 16-bit PCM in.
        let audio = match audio_channels {
            Some(ch) => {
                let ch = ch.clamp(1, 2);
                let out = audio_type(mf, ch, true).map_err(fail("audio type"))?;
                let stream = unsafe { writer.AddStream(&out) }.map_err(fail("AddStream audio"))?;
                let input = audio_type(mf, ch, false).map_err(fail("audio type"))?;
                unsafe { writer.SetInputMediaType(stream, &input, None) }
                    .map_err(fail("SetInputMediaType audio"))?;
                Some((stream, ch))
            }
            None => None,
        };

        // NV12 textures the video processor can write and the encoder reads,
        // recycled when the encoder lets go of a sample.
        let allocator: IMFVideoSampleAllocatorEx = created(|p| unsafe {
            (mf.create_video_sample_allocator_ex)(&IMFVideoSampleAllocatorEx::IID, p)
        })
        .map_err(fail("MFCreateVideoSampleAllocatorEx"))?;
        let alloc_attrs = new_attributes(mf, 1).map_err(fail("MFCreateAttributes"))?;
        set32(
            &alloc_attrs,
            MF_SA_D3D11_BINDFLAGS,
            D3D11_BIND_RENDER_TARGET.0 as u32,
        )
        .map_err(fail("allocator"))?;
        unsafe {
            allocator
                .SetDirectXManager(manager)
                .map_err(fail("SetDirectXManager"))?;
            allocator
                .InitializeSampleAllocatorEx(4, 16, &alloc_attrs, &input)
                .map_err(fail("InitializeSampleAllocatorEx"))?;
        }

        unsafe { writer.BeginWriting() }.map_err(no_encoder)?;
        let info = encoder_info(&writer, video);
        Ok(Encoder {
            mf,
            writer,
            video,
            audio,
            allocator,
            frame_hns: 10_000_000 / spec.fps.max(1) as i64,
            info,
        })
    }

    /// A hardware encoder without a friendly name is named after the GPU.
    pub fn name_adapter(&mut self, adapter: &str) {
        if self.info.hardware && self.info.name == GENERIC_HARDWARE && !adapter.is_empty() {
            self.info.name = format!("{GENERIC_HARDWARE} on {adapter}");
        }
    }

    pub fn info(&self) -> &EncoderInfo {
        &self.info
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// A free NV12 frame to render into: the sample, its texture and the
    /// texture's subresource. `None` when the encoder still holds them all
    /// (the frame is dropped).
    pub fn next_frame(&self) -> Result<Option<(IMFSample, ID3D11Texture2D, u32)>, Error> {
        let sample = match unsafe { self.allocator.AllocateSample() } {
            Ok(s) => s,
            Err(e) if e.code() == MF_E_SAMPLEALLOCATOR_EMPTY => return Ok(None),
            Err(e) => return Err(Error::win("AllocateSample", &e)),
        };
        let buffer = unsafe { sample.GetBufferByIndex(0) }
            .map_err(|e| Error::win("GetBufferByIndex", &e))?;
        let dxgi: IMFDXGIBuffer = buffer.cast().map_err(|e| Error::win("IMFDXGIBuffer", &e))?;
        let mut raw = std::ptr::null_mut();
        unsafe { dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw) }
            .map_err(|e| Error::win("GetResource", &e))?;
        // SAFETY: GetResource handed over one reference to the texture.
        let texture = unsafe { ID3D11Texture2D::from_raw(raw) };
        let sub = unsafe { dxgi.GetSubresourceIndex() }.unwrap_or(0);
        // Software encoders lock the buffer: it must say how long it is.
        if let Ok(b2) = buffer.cast::<IMF2DBuffer>()
            && let Ok(len) = unsafe { b2.GetContiguousLength() }
        {
            let _ = unsafe { buffer.SetCurrentLength(len) };
        }
        Ok(Some((sample, texture, sub)))
    }

    /// Writes a rendered frame at `time` (100 ns).
    pub fn write_frame(&self, sample: &IMFSample, time: i64) -> Result<(), Error> {
        unsafe {
            sample
                .SetSampleTime(time)
                .and_then(|_| sample.SetSampleDuration(self.frame_hns))
                .and_then(|_| self.writer.WriteSample(self.video, sample))
        }
        .map_err(|e| Error::win("WriteSample video", &e))
    }

    /// Writes interleaved 16-bit PCM (the stream's channel count) at `time`.
    pub fn write_audio(&self, pcm: &[i16], time: i64) -> Result<(), Error> {
        let Some((stream, ch)) = self.audio else {
            return Ok(());
        };
        if pcm.is_empty() {
            return Ok(());
        }
        let bytes = pcm.len() * 2;
        let frames = (pcm.len() / ch as usize) as u64;
        let write = || -> windows::core::Result<()> {
            let buffer: IMFMediaBuffer =
                created(|p| unsafe { (self.mf.create_memory_buffer)(bytes as u32, p) })?;
            unsafe {
                let mut data = std::ptr::null_mut();
                buffer.Lock(&mut data, None, None)?;
                std::ptr::copy_nonoverlapping(pcm.as_ptr().cast::<u8>(), data, bytes);
                buffer.Unlock()?;
                buffer.SetCurrentLength(bytes as u32)?;
                let sample: IMFSample = created(|p| (self.mf.create_sample)(p))?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(time)?;
                sample.SetSampleDuration(frames_hns(frames))?;
                self.writer.WriteSample(stream, &sample)
            }
        };
        write().map_err(|e| Error::win("WriteSample audio", &e))
    }

    /// Tells the muxer the audio has a gap up to `time` (no audio came), so
    /// it does not hold the video back waiting for it.
    pub fn audio_gap(&self, time: i64) {
        if let Some((stream, _)) = self.audio {
            let _ = unsafe { self.writer.SendStreamTick(stream, time) };
        }
    }

    /// Ends the file (writes the MP4 index).
    pub fn finish(self) -> Result<(), Error> {
        unsafe { self.writer.Finalize() }.map_err(|e| Error::win("Finalize", &e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_rates_pack_high_then_low() {
        assert_eq!(pack(1920, 1080), 0x0000_0780_0000_0438);
        assert_eq!(pack(30, 1), (30u64 << 32) | 1);
    }

    #[test]
    fn aac_takes_its_fixed_rates() {
        assert_eq!(aac_bytes_per_second(2), 16_000);
        assert_eq!(aac_bytes_per_second(1), 12_000);
    }
}
