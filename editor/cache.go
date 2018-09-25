package editor

import (
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/boltdb/bolt"
	"github.com/dzhou121/crane/log"
)

// Cache is
type Cache struct {
	db *bolt.DB
}

func newCache(e *Editor) *Cache {
	db, err := bolt.Open(filepath.Join(e.config.configDir, "cache"), 0600, nil)
	if err != nil {
		return nil
	}
	return &Cache{
		db: db,
	}
}

func (c *Cache) setLastPosition(loc *Location) {
	c.db.Update(func(tx *bolt.Tx) error {
		path := loc.path
		bkt, err := tx.CreateBucketIfNotExists([]byte(path))
		if err != nil {
			log.Infoln(err)
			return err
		}
		result, err := json.Marshal(loc)
		if err != nil {
			log.Infoln(err)
			return err
		}
		bkt.Put([]byte("location"), result)
		return nil
	})
}

func (c *Cache) getLastPosition(path string) (*Location, error) {
	tx, err := c.db.Begin(true)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()

	bkt := tx.Bucket([]byte(path))
	if bkt == nil {
		return nil, errors.New("no such bkt")
	}
	result := bkt.Get([]byte("location"))
	log.Infoln(string(result))
	var loc Location
	err = json.Unmarshal(result, &loc)
	if err != nil {
		return nil, err
	}
	return &loc, nil
}
