package plugin

import (
	"encoding/json"
	"net"

	"github.com/crane-editor/crane/log"
)

// ConnStream is
type ConnStream struct {
	conn    net.Conn
	decoder *json.Decoder
	encoder *json.Encoder
}

// NewConnStream creates
func NewConnStream(conn net.Conn) *ConnStream {
	return &ConnStream{
		conn:    conn,
		decoder: json.NewDecoder(conn),
		encoder: json.NewEncoder(conn),
	}
}

// WriteObject implements ObjectStream.
func (s *ConnStream) WriteObject(obj interface{}) error {
	return s.encoder.Encode(obj)
}

// ReadObject implements ObjectStream.
func (s *ConnStream) ReadObject(v interface{}) error {
	err := s.decoder.Decode(v)
	if err != nil {
		log.Infoln(err)
	}
	return err
}

// Close implements ObjectStream.
func (s *ConnStream) Close() error {
	return s.conn.Close()
}
