sozcuklemece


Tarayıcılar üzerinden oynanılan günlük oyunların arttığı bu dönemde yeni ve bambaşka bir bulmaca

BU günlük bulmacada kullanıclar verilen karşılarına çıkan yepyeni türetme sözcüğün anlamını tahmin etmeye çalışıyorlar.
oyun şöyle olacak. 3 dakikalık bir sayaç ve bu sürede bulmaya çalışan bir oyuncu var. Ne kadar erken bulursa o kadar iyi, kullanılan ipuçları kazandığı puanı azaltıyor.
İpuçlar:
    kelimenin kökündeki sözcük(ler), ek(ler) 
    cümlede kullanımı
    Kategori ve ilgili olduğu alan (bilim, yemek,siyaset, his)
her gün 1 yeni sözcükle yarışma oluyor.


Buradaki asıl olay şu, Sözcüğün tanımı 1 tane, ama kullanıcıların yazabilecekleri tanımlar sonsuz tane olduğu için iki cümleyi kıyaslayabilecek bir yönteme ihtiyacımız var.
Burada aynı zamanda çokça kullanıcının siteye gireceği kalabalık zamanları da düşündüğümüz için bu yöntemin çok hızlı olması gerek. Bunun için benim aklımaasıl tanım ile kullanıcı tahminini word2vec'ten geçirip, her birini kendi vektörleri ile topladıktan sonra uzaklıklarını hesaplamak geldi. Bu fikir nasıl sence? Bundan daha iyi ve hız/doğruluk performansı olarak daha iy ibir yöntem geliyorsa aklına söyle
 FastAPI ile yapmanın iyi olacağını düşünüyorum, üyelik gerekmiyor ve yz kullanacağız, bu yüzden hız çok önemli.
 

## Çalıştırma

Arka uç Rust ile yazıldı, modeli [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference) (TEI) çalıştırır.

    docker run --rm -p 127.0.0.1:8080:80 ghcr.io/huggingface/text-embeddings-inference:cpu-1.9 --model-id emrecan/bert-base-turkish-cased-mean-nli-stsb-tr
    cd backend
    ADMIN_TOKEN=uzun-rastgele-bir-parola cargo run --release

Arka uç `TEI_URL` (varsayılan `http://127.0.0.1:8080`), `BIND_ADDRESS` (varsayılan `0.0.0.0:8000`) ve `WORD_FILE` (varsayılan `data/current_word.json`) ortam değişkenlerini okur.

Testler: `cd backend && cargo test`

## Günün sözcüğünü yükleme

`/admin/` sayfasını aç, `ADMIN_TOKEN` parolasını gir, formu doldurup yayınla. `ADMIN_TOKEN` ayarlı değilse yükleme kapalıdır. Parola düz metin gittiği için canlıda HTTPS kullan.

- Sözcüğün işlenmiş hali yüklenirken hazırlanır ve bellekte durur, tahminlerde yeniden hesaplanmaz.
- Yeni sözcük hemen oyuna girer, önceki sözcük bellekten ve diskten silinir. O sırada süren oyunlar kendi sözcükleriyle biter.
- Sözcük yüklendikten 24 saat sonra kendiliğinden silinir. Yenisi yüklenene kadar oyun başlatılamaz.
- Sunucu yeniden başlarsa sözcük `WORD_FILE` dosyasından okunup yeniden hazırlanır. Bu dosya depoya girmez.

Doğruluğu örnek sözcüklerle ölçmek için TEI açıkken `backend` klasöründe:

    cargo run --release --bin evaluate_guesses
    cargo test -- --ignored

Oyun ayarları: kazanma eşiği `backend/src/similarity.rs`, süre ve puan `backend/src/game.rs`, sözcüğün ömrü `backend/src/daily_word.rs`, istek sınırları `backend/src/rate_limit.rs`.

## İstek sınırı

Oyun başlatma, tahmin ve yönetim istekleri IP başına sınırlı. Sınır aşılınca sunucu 429 ve `Retry-After` döner. Sayaçlar oyunlar gibi süreç belleğinde tutuluyor, bu yüzden arka uç tek kopya çalışır.

Arka uç bir ters vekilin (nginx vb.) arkasındaysa `CLIENT_IP_HEADER=X-Real-IP` ver, yoksa bütün oyuncular tek IP sayılır. Bu başlığa yalnızca vekil dışarıya kapalıyken güven, yoksa oyuncular IP'lerini taklit edebilir.
