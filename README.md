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
 

## Mimari

Üç kapsayıcı:

- `frontend`: nginx. Statik dosyaları sunar, `/api/` isteklerini arka uca iletir. Dışarıya açık tek kapsayıcı budur.
- `backend`: Rust arka uç. Oyunlar, puanlar, istek sınırı ve günün sözcüğü burada.
- `ai`: [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference) (TEI). Modeli ONNX olarak çalıştırır, gelen istekleri kuyruğa alıp toplu işler. Model imaja gömülüdür, açılışta internete çıkmaz.

## Çalıştırma

    ADMIN_TOKEN=uzun-rastgele-bir-parola docker compose up -d --build

Ardından http://127.0.0.1:8000 adresini aç. İlk derleme modeli indirip ONNX'e çevirdiği için birkaç dakika sürer. Günün sözcüğü `word-data` biriminde durur, yeniden başlatmada kaybolmaz.

Arka ucu kapsayıcı dışında çalıştırmak için:

    docker compose build ai
    docker run --rm -p 127.0.0.1:8080:80 sozcuklemece-ai
    cd backend
    ADMIN_TOKEN=uzun-rastgele-bir-parola cargo run --release

Arka uç `TEI_URL` (varsayılan `http://127.0.0.1:8080`), `BIND_ADDRESS` (varsayılan `0.0.0.0:8000`) ve `WORD_FILE` (varsayılan `data/current_word.json`) ortam değişkenlerini okur.

Testler: `cd backend && cargo test`

## Önbellek ve CDN

nginx ön yüz dosyalarını önbellek başlıklarıyla sunar: HTML, JS ve CSS 5 dakika, yazı tipleri ve sesler 30 gün. `/api/` yanıtları hiç önbelleğe alınmaz. Önüne bir CDN (ör. Cloudflare) konursa nginx'e `set_real_ip_from` ve `real_ip_header` eklenmeli, yoksa bütün oyuncular CDN'in IP'si sayılır.

## Günün sözcüğünü yükleme

`/admin/` sayfasını aç, `ADMIN_TOKEN` parolasını gir, formu doldurup yayınla. `ADMIN_TOKEN` ayarlı değilse yükleme kapalıdır. Parola düz metin gittiği için canlıda HTTPS kullan.

- Sözcüğün işlenmiş hali yüklenirken hazırlanır ve bellekte durur, tahminlerde yeniden hesaplanmaz.
- Yeni sözcük hemen oyuna girer, önceki sözcük bellekten ve diskten silinir. O sırada süren oyunlar kendi sözcükleriyle biter.
- Sözcük yüklendikten 24 saat sonra kendiliğinden silinir. Yenisi yüklenene kadar oyun başlatılamaz.
- Sunucu yeniden başlarsa sözcük `WORD_FILE` dosyasından okunup yeniden hazırlanır. Bu dosya depoya girmez.
- Tahminlerin vektörleri en çok 1000 tahminlik bir önbellekte tutulur, az kullanılanlar yenilerine yer açar. Aynı anda gelen aynı tahmin modele bir kez gider. Sözcük değişince ya da süresi dolunca önbellek boşaltılır.

Doğruluğu örnek sözcüklerle ölçmek için TEI açıkken `backend` klasöründe:

    cargo run --release --bin evaluate_guesses
    cargo test -- --ignored

Oyun ayarları: kazanma eşiği ve önbellek boyutu `backend/src/similarity.rs`, süre ve puan `backend/src/game.rs`, sözcüğün ömrü `backend/src/daily_word.rs`, istek sınırları `backend/src/rate_limit.rs`.

## İstek sınırı

Oyun başlatma, tahmin ve yönetim istekleri IP başına sınırlı. Sınır aşılınca sunucu 429 ve `Retry-After` döner. Sayaçlar oyunlar gibi süreç belleğinde tutuluyor, bu yüzden arka uç tek kopya çalışır.

Arka uç oyuncunun IP'sini nginx'in eklediği `X-Real-IP` başlığından okur (`CLIENT_IP_HEADER`). Bu yüzden `backend` kapsayıcısının portu dışarıya açılmamalı, yoksa oyuncular IP'lerini taklit edebilir.
