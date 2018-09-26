package lsp

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os/exec"

	"github.com/dzhou121/crane/log"
)

// StdinoutStream is
type StdinoutStream struct {
	in     io.WriteCloser
	out    io.ReadCloser
	reader *bufio.Reader
}

// NewStdinoutStream creates
func NewStdinoutStream(command string, arg ...string) (*StdinoutStream, error) {
	cmd := exec.Command(command, arg...)
	inw, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}

	outr, err := cmd.StdoutPipe()
	if err != nil {
		inw.Close()
		return nil, err
	}

	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	go func() {
		buf := make([]byte, 1000)
		for {
			n, err := stderr.Read(buf)
			if err != nil {
				return
			}
			log.Infoln("stderr:", string(buf[:n]))
		}
	}()

	err = cmd.Start()
	if err != nil {
		return nil, err
	}
	return &StdinoutStream{
		in:     inw,
		out:    outr,
		reader: bufio.NewReaderSize(outr, 8192),
	}, nil
}

// WriteObject implements ObjectStream.
func (s *StdinoutStream) WriteObject(obj interface{}) error {
	data, err := json.Marshal(obj)
	if err != nil {
		return err
	}
	s.in.Write([]byte(fmt.Sprintf("Content-Length: %d\r\n\r\n", len(data))))
	_, err = s.in.Write(data)
	return err
}

// ReadObject implements ObjectStream.
func (s *StdinoutStream) ReadObject(v interface{}) error {
	s.reader.ReadSlice('\n')
	s.reader.ReadSlice('\n')
	decoder := json.NewDecoder(s.reader)
	err := decoder.Decode(v)
	if err != nil {
		log.Infoln(err)
	}
	return err

	// s.reader.ReadSlice('\n')
	// s.reader.ReadSlice('\n')
	// buf := make([]byte, 118096)
	// n, err := s.reader.Read(buf)
	// if err != nil {
	// 	return nil
	// }
	// log.Infoln(string(buf[:n]))
	// err = json.Unmarshal(buf[:n], v)
	// if err != nil {
	// 	log.Infoln(err)
	// }
	// return err
}

// Close implements ObjectStream.
func (s *StdinoutStream) Close() error {
	err := s.in.Close()
	if err != nil {
		return err
	}
	return s.out.Close()
}
