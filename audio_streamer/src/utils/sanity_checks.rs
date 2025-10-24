fn test_cpal() {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(AudioRecordingError::DefaultOutputDeviceError()).unwrap();
    let config = device.default_output_config().unwrap();

    let spec = hound::WavSpec {
        channels: config.channels().clone(),
        sample_rate: config.sample_rate().0.clone(),
        bits_per_sample: 32, // We are working with f32
        sample_format: hound::SampleFormat::Float,
    };

    let (mut producer, mut consumer) = ring_buffer::<8192>();

    let err_fn = |err| eprintln!("Stream error: {}", err);
    let audio_stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(), 
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let pushed = producer.push_slice(data);
            }, 
            err_fn, 
            None
        ),

        _ => unimplemented!("Only F32 supported in this example")
    }.unwrap();

    audio_stream.play().unwrap();
    println!("Capturing the audio..");

    let stop_signal = Arc::new(AtomicBool::new(false));

    // Create a clone of the Arc to be moved into the thread.
    // The main thread will keep the original.
    let signal_clone = Arc::clone(&stop_signal);
    let consumer_handle = thread::spawn(move || {
        let mut buffer: [f32; 8192] = [0.0; 8192];

        let mut big_buffer: Vec<f32> = Vec::with_capacity(48000 * 10);
        
        while !signal_clone.load(Ordering::SeqCst) {
            let popped_amount = consumer.pop_slice(&mut buffer);
            if popped_amount > 0 {
                big_buffer.extend_from_slice(&buffer[..popped_amount]);
            } else {
                // Sleep briefly if there's no data to avoid busy-waiting.
                thread::sleep(Duration::from_millis(1));
            }
        }

        big_buffer
    });

    io::stdin().read_line(&mut String::new()).unwrap();
    stop_signal.store(true, Ordering::SeqCst);

    let captured_pcm_f32 = consumer_handle.join().expect("Consumer thread panicked");

    let mut writer = hound::WavWriter::create("capture_test.wav", spec).unwrap();
    for sample in captured_pcm_f32 {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();

    println!("Saved captured audio to 'capture_test.wav'");

}

fn test_encoder() {
    let mut reader = hound::WavReader::open("capture_test.wav").unwrap();
    let spec = reader.spec();

    if spec.sample_format != hound::SampleFormat::Float || spec.bits_per_sample != 32 {
        eprintln!("WAV file is not 32-bit float PCM. Please check the capture format.");
    }

    let all_pcm_samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
    println!("Read {} total samples from WAV file.", all_pcm_samples.len());

    let channels = match spec.channels {
        1 => opusic_c::Channels::Mono,
        2 => opusic_c::Channels::Stereo,
        _ => {
            eprintln!("Unsupported number of channels: {}", spec.channels);
            opusic_c::Channels::Stereo
        },
    };
    let sample_rate = match spec.sample_rate {
        48000 => opusic_c::SampleRate::Hz48000,
        _ => {
            eprintln!("Unsupported sample rate: {}", spec.sample_rate);
            opusic_c::SampleRate::Hz48000
        },
    };

    let mut encoder = opusic_c::Encoder::new(
        channels,
        sample_rate,
        opusic_c::Application::Audio,
    ).unwrap();

    const FRAME_SIZE_SAMPLES: usize = 960;
    let frame_length_interleaved = FRAME_SIZE_SAMPLES * spec.channels as usize;

    let mut output_file = File::create("encoded.opus").unwrap();
    let mut encoded_frames_count = 0;

    for pcm_frame in all_pcm_samples.chunks_exact(frame_length_interleaved) {
        let mut encoded_opus_frame: Vec<u8> = Vec::with_capacity(frame_length_interleaved);
        
        let bytes_encoded = encoder.encode_float_to_vec(pcm_frame, &mut encoded_opus_frame).unwrap();

        let frame_len_u32 = bytes_encoded as u32;

        let len_bytes = frame_len_u32.to_be_bytes();

        output_file.write_all(&len_bytes).unwrap();

        output_file.write_all(&encoded_opus_frame).unwrap();
        
        encoded_frames_count += 1;
    }

    println!("Successfully encoded {} frames to '{}'", encoded_frames_count, "encoded.opus");
    
}

fn test_decoder() {
    let wav_reader = hound::WavReader::open("capture_test.wav").unwrap();
    let spec = wav_reader.spec();

    if spec.sample_format != hound::SampleFormat::Float || spec.bits_per_sample != 32 {
        eprintln!("WAV file is not 32-bit float PCM. Please check the capture format.");
    }

    let channels = match spec.channels {
        1 => opusic_c::Channels::Mono,
        2 => opusic_c::Channels::Stereo,
        _ => {
            eprintln!("Unsupported number of channels: {}", spec.channels);
            opusic_c::Channels::Stereo
        },
    };
    let sample_rate = match spec.sample_rate {
        48000 => opusic_c::SampleRate::Hz48000,
        _ => {
            eprintln!("Unsupported sample rate: {}", spec.sample_rate);
            opusic_c::SampleRate::Hz48000
        },
    };

    let mut decoded_file = File::open("encoded.opus").unwrap();
    let mut writer = hound::WavWriter::create("decoded.wav", spec).unwrap();
    let mut decoder = opusic_c::Decoder::new(channels, sample_rate).unwrap();
    let mut decoded_frames_count = 0;
    let mut len_buffer = [0u8; 4];

    while decoded_file.read_exact(&mut len_buffer).is_ok() {
        let frame_len = u32::from_be_bytes(len_buffer);

        let mut opus_frame_buffer = vec![0u8; frame_len as usize];
        decoded_file.read_exact(&mut opus_frame_buffer).unwrap();

        let mut pcm_output_buffer = [0.0f32; 1920]; 
        decoder.decode_float_to_slice(&opus_frame_buffer, &mut pcm_output_buffer, false).unwrap();

        for sample in &pcm_output_buffer {
            writer.write_sample(*sample).unwrap();
        }
        
        decoded_frames_count += 1;
    }

    writer.finalize().unwrap();
    println!("Successfully decoded {} frames and saved to '{}'", decoded_frames_count, "decoded.wav");
}