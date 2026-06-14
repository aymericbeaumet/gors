package main

import (
	"crypto"
	"fmt"
	"hash"
	"io"
)

type fakeHash struct {
	data []byte
}

type fakeReader struct{}

func (fakeReader) Read(p []byte) (int, error) {
	return 0, nil
}

func (h *fakeHash) Write(p []byte) (int, error) {
	h.data = append(h.data, p...)
	return len(p), nil
}

func (h *fakeHash) Sum(b []byte) []byte {
	out := append([]byte{}, b...)
	return append(out, byte(len(h.data)), byte(h.Size()))
}

func (h *fakeHash) Reset() {
	h.data = nil
}

func (h *fakeHash) Size() int {
	return 2
}

func (h *fakeHash) BlockSize() int {
	return 4
}

func newFakeHash() hash.Hash {
	return &fakeHash{}
}

type fakePublic struct {
	id int
}

type fakeSigner struct {
	id int
}

func (s fakeSigner) Public() crypto.PublicKey {
	return fakePublic{s.id}
}

func (s fakeSigner) Sign(rand io.Reader, digest []byte, opts crypto.SignerOpts) ([]byte, error) {
	return []byte{byte(s.id), byte(len(digest)), byte(opts.HashFunc())}, nil
}

type fakeMessageSigner struct {
	fakeSigner
}

func (s fakeMessageSigner) SignMessage(rand io.Reader, msg []byte, opts crypto.SignerOpts) ([]byte, error) {
	return []byte{byte(s.id + 100), byte(len(msg)), byte(opts.HashFunc())}, nil
}

type fakeDecrypter struct {
	id int
}

func (d fakeDecrypter) Public() crypto.PublicKey {
	return fakePublic{d.id}
}

func (d fakeDecrypter) Decrypt(rand io.Reader, msg []byte, opts crypto.DecrypterOpts) ([]byte, error) {
	label, _ := opts.(string)
	return []byte{byte(d.id), byte(len(msg)), byte(len(label))}, nil
}

type fakeEncapsulator struct {
	id byte
}

func (e fakeEncapsulator) Bytes() []byte {
	return []byte{e.id, e.id + 1}
}

func (e fakeEncapsulator) Encapsulate() ([]byte, []byte) {
	return []byte{e.id + 2}, []byte{e.id + 3, e.id + 4}
}

type fakeDecapsulator struct {
	enc fakeEncapsulator
}

func (d fakeDecapsulator) Encapsulator() crypto.Encapsulator {
	return d.enc
}

func (d fakeDecapsulator) Decapsulate(ciphertext []byte) ([]byte, error) {
	return []byte{d.enc.id, byte(len(ciphertext))}, nil
}

func caseCryptoHashConstants() {
	// gors:stdlib-cover crypto::Hash
	// gors:stdlib-cover crypto::BLAKE2b_256
	// gors:stdlib-cover crypto::BLAKE2b_384
	// gors:stdlib-cover crypto::BLAKE2b_512
	// gors:stdlib-cover crypto::BLAKE2s_256
	// gors:stdlib-cover crypto::MD4
	// gors:stdlib-cover crypto::MD5
	// gors:stdlib-cover crypto::MD5SHA1
	// gors:stdlib-cover crypto::RIPEMD160
	// gors:stdlib-cover crypto::SHA1
	// gors:stdlib-cover crypto::SHA224
	// gors:stdlib-cover crypto::SHA256
	// gors:stdlib-cover crypto::SHA384
	// gors:stdlib-cover crypto::SHA3_224
	// gors:stdlib-cover crypto::SHA3_256
	// gors:stdlib-cover crypto::SHA3_384
	// gors:stdlib-cover crypto::SHA3_512
	// gors:stdlib-cover crypto::SHA512
	// gors:stdlib-cover crypto::SHA512_224
	// gors:stdlib-cover crypto::SHA512_256
	fmt.Println(
		int(crypto.MD4),
		int(crypto.MD5),
		int(crypto.SHA1),
		int(crypto.SHA224),
		int(crypto.SHA256),
		int(crypto.SHA384),
		int(crypto.SHA512),
		int(crypto.MD5SHA1),
		int(crypto.RIPEMD160),
		int(crypto.SHA3_224),
		int(crypto.SHA3_256),
		int(crypto.SHA3_384),
		int(crypto.SHA3_512),
		int(crypto.SHA512_224),
		int(crypto.SHA512_256),
		int(crypto.BLAKE2s_256),
		int(crypto.BLAKE2b_256),
		int(crypto.BLAKE2b_384),
		int(crypto.BLAKE2b_512),
	)
}

func caseCryptoHashMethods() {
	// gors:stdlib-cover crypto::Hash.HashFunc
	// gors:stdlib-cover crypto::Hash.Available
	// gors:stdlib-cover crypto::Hash.New
	// gors:stdlib-cover crypto::Hash.Size
	// gors:stdlib-cover crypto::Hash.String
	// gors:stdlib-cover crypto::RegisterHash
	fmt.Println(
		crypto.SHA256.HashFunc() == crypto.SHA256,
		crypto.SHA256.Size(),
		crypto.SHA256.String(),
	)

	fmt.Println("hash-registry-before", crypto.MD4.Available())
	crypto.RegisterHash(crypto.MD4, newFakeHash)
	h := crypto.MD4.New()
	n, err := h.Write([]byte{1, 2, 3})
	sum := h.Sum([]byte{9})
	fmt.Println("hash-registry-after", crypto.MD4.Available(), n, err == nil, h.Size(), h.BlockSize(), len(sum), int(sum[0]), int(sum[1]), int(sum[2]))
	h.Reset()
	empty := h.Sum(nil)
	fmt.Println("hash-registry-reset", len(empty), int(empty[0]), int(empty[1]), crypto.MD4.HashFunc() == crypto.MD4, crypto.MD4.String())
}

func caseCryptoKeyInterfaces() {
	// gors:stdlib-cover crypto::PublicKey
	// gors:stdlib-cover crypto::PrivateKey
	pub := crypto.PublicKey(fakePublic{7})
	priv := crypto.PrivateKey(fakeSigner{7})
	fmt.Println("key-aliases", pub.(fakePublic).id == 7, priv.(fakeSigner).Public().(fakePublic).id == pub.(fakePublic).id)

	// gors:stdlib-cover crypto::Signer
	// gors:stdlib-cover crypto::SignerOpts
	var signer crypto.Signer = fakeSigner{11}
	var opts crypto.SignerOpts = crypto.MD4
	sig, signErr := fakeSigner{11}.Sign(fakeReader{}, []byte{1, 2, 3}, crypto.MD4)
	fmt.Println("signer", signer.Public().(fakePublic).id == 11, signErr == nil, len(sig), int(sig[0]), int(sig[1]), int(sig[2]), opts.HashFunc().String())

	// gors:stdlib-cover crypto::MessageSigner
	// gors:stdlib-cover crypto::SignMessage
	fallbackSig, fallbackErr := crypto.SignMessage(fakeSigner{11}, fakeReader{}, []byte{4, 5, 6, 7}, crypto.MD4)
	directSig, directErr := crypto.SignMessage(fakeMessageSigner{fakeSigner{13}}, fakeReader{}, []byte{8, 9}, crypto.Hash(0))
	fmt.Println("sign-message", fallbackErr == nil, len(fallbackSig), int(fallbackSig[0]), int(fallbackSig[1]), int(fallbackSig[2]), directErr == nil, len(directSig), int(directSig[0]), int(directSig[1]), int(directSig[2]))

	// gors:stdlib-cover crypto::Decrypter
	// gors:stdlib-cover crypto::DecrypterOpts
	var decrypter crypto.Decrypter = fakeDecrypter{17}
	plain, decryptErr := fakeDecrypter{17}.Decrypt(fakeReader{}, []byte{1, 2, 3, 4}, "label")
	fmt.Println("decrypter", decrypter.Public().(fakePublic).id == 17, decryptErr == nil, len(plain), int(plain[0]), int(plain[1]), int(plain[2]))
}

func caseCryptoKemInterfaces() {
	// gors:stdlib-cover crypto::Encapsulator
	var enc crypto.Encapsulator = fakeEncapsulator{21}
	encBytes := enc.Bytes()
	shared, ciphertext := enc.Encapsulate()
	fmt.Println("encapsulator", len(encBytes), int(encBytes[0]), int(encBytes[1]), len(shared), int(shared[0]), len(ciphertext), int(ciphertext[0]), int(ciphertext[1]))

	// gors:stdlib-cover crypto::Decapsulator
	var dec crypto.Decapsulator = fakeDecapsulator{fakeEncapsulator{31}}
	decEnc := dec.Encapsulator()
	decBytes := decEnc.Bytes()
	decShared, decErr := dec.Decapsulate([]byte{1, 2, 3})
	fmt.Println("decapsulator", len(decBytes), int(decBytes[0]), int(decBytes[1]), decErr == nil, len(decShared), int(decShared[0]), int(decShared[1]))
}

func main() {
	caseCryptoHashConstants()
	caseCryptoHashMethods()
	caseCryptoKeyInterfaces()
	caseCryptoKemInterfaces()
}
