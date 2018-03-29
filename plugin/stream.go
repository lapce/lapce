package plugin

import (
	"encoding/json"
	"io"
	"log"
	"os"
)

// StdinoutStream is
type StdinoutStream struct {
	in      io.WriteCloser
	out     io.ReadCloser
	decoder *json.Decoder
	encoder *json.Encoder
}

// NewStdinoutStream creates
func NewStdinoutStream() *StdinoutStream {
	return &StdinoutStream{
		in:      os.Stdout,
		out:     os.Stdin,
		decoder: json.NewDecoder(os.Stdin),
		encoder: json.NewEncoder(os.Stdout),
	}
}

// WriteObject implements ObjectStream.
func (s *StdinoutStream) WriteObject(obj interface{}) error {
	err := s.encoder.Encode(obj)
	s.in.Write([]byte{'\n'})
	return err
}

// ReadObject implements ObjectStream.
func (s *StdinoutStream) ReadObject(v interface{}) error {
	err := s.decoder.Decode(v)
	if err != nil {
		log.Println(err)
	}
	log.Println(v)
	return err
}

// Close implements ObjectStream.
func (s *StdinoutStream) Close() error {
	return nil
}
