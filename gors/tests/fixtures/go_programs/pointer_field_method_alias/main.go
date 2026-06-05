package main

import "os"

type bucket struct {
	total int
}

func (b *bucket) fill(p *int) {
	*p = 9
	b.total += *p
}

type holder struct {
	bucket bucket
	value  int
}

func update(h *holder) {
	h.bucket.fill(&h.value)
}

func main() {
	h := &holder{}
	update(h)
	if h.value == 9 && h.bucket.total == 9 {
		os.Stdout.Write([]byte("ok\n"))
		return
	}
	os.Stdout.Write([]byte("bad\n"))
}
