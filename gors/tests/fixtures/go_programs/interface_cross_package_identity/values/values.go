package values

import "example/contracts"

type Store map[string]int

func NewStore(value int) Store {
	store := make(Store)
	store["value"] = value
	return store
}

func (store Store) Read() int {
	return store["value"]
}

func (store Store) Write(value int) {
	store["value"] = value
}

func AsReader(store Store) contracts.Reader {
	return store
}
