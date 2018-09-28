package plugin

import (
	"encoding/json"
	"io"
	"os"

	"github.com/crane-editor/crane/log"
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
	data, err := json.Marshal(obj)
	if err != nil {
		return err
	}
	data = append(data, '\n')
	log.Infoln(string(data))
	_, err = s.in.Write(data)
	return err
}

// ReadObject implements ObjectStream.
func (s *StdinoutStream) ReadObject(v interface{}) error {
	err := s.decoder.Decode(v)
	if err != nil {
		log.Infoln(err)
	}
	return err
}

// Close implements ObjectStream.
func (s *StdinoutStream) Close() error {
	return nil
}
