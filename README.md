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

Dört kapsayıcı:

- `frontend`: nginx. Statik dosyaları sunar, `/api/` isteklerini arka uca iletir. Dışarıya açık tek kapsayıcı budur.
- `backend`: Rust arka uç. Oyunlar, puanlar, istek sınırı ve günün sözcüğü burada.
- `ai-1`, `ai-2`: [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference) (TEI). Modeli int8'e indirilmiş ONNX olarak çalıştırır, gelen istekleri kuyruğa alıp toplu işler. Model imaja gömülüdür, açılışta internete çıkmaz. Arka uç istekleri iki kopyaya sırayla dağıtır, biri bağlantıyı reddederse ötekine gider. Her kopyanın CPU sınırı `.env` içindeki `AI_CPUS` ile verilir (varsayılan 2). TEI iş parçacıklarını açılıştaki CPU sayısına göre kurduğu için sınır sonradan `docker update` ile değil, kapsayıcı açılırken verilmeli. Bu makinede iki 2 CPU'luk kopya, tek 4 CPU'luk kopyadan yaklaşık %15 daha çok istek işledi.

## Çalıştırma

    cp .env.example .env
    docker compose up -d --build

`.env` dosyasında `ADMIN_TOKEN`'ı uzun, rastgele bir parolayla değiştir. Bu dosya depoya girmez, Compose onu kendiliğinden okur.

Ardından http://127.0.0.1:8000 adresini aç. İlk derleme modeli indirip ONNX'e çevirdiği için birkaç dakika sürer. Günün sözcüğü `word-data` biriminde durur, yeniden başlatmada kaybolmaz.

Açılış sırası: iki `ai` kopyası sağlıklı olunca `backend`, `backend` hazır olunca `frontend` başlar. `backend` kayıtlı sözcüğü yükleyene kadar istek kabul etmez. nginx arka ucun adresini 10 saniyede bir yeniden çözer, bu yüzden yalnızca `backend` yeniden kurulduğunda da ön yüz çalışmaya devam eder.

Arka ucu kapsayıcı dışında çalıştırmak için:

    docker compose build ai-1
    docker run --rm -p 127.0.0.1:8080:80 sozcuklemece-ai
    cd backend
    set -a && . ../.env && set +a
    cargo run --release

Arka uç `TEI_URL` (varsayılan `http://127.0.0.1:8080`, birden çok kopya için virgülle ayrılır), `BIND_ADDRESS` (varsayılan `0.0.0.0:8000`) ve `WORD_FILE` (varsayılan `data/current_word.json`) ortam değişkenlerini okur.

Testler: `cd backend && cargo test`

## İnsan doğrulaması

Oyun başlatılırken Cloudflare Turnstile ile doğrulama yapılır. Görünmez çalışır, oyuncuya kutucuk yalnızca Cloudflare şüphelenirse gösterilir. Turnstile jetonu tek kullanımlık ve her doğrulama Cloudflare'e bir istek olduğu için doğrulama her tahminde değil oyun başında yapılır. Sonraki istekler tahmin edilemeyen oyun kimliğine ve oyun başı bekleme süresine bağlıdır.

`TURNSTILE_SITE_KEY`, `TURNSTILE_SECRET_KEY` ve `TURNSTILE_HOSTNAME` zorunludur. Anahtarlar Cloudflare panelinden alınır, `TURNSTILE_HOSTNAME` sitenin alan adıdır (ör. `sozcuklemece.com`). Arka uç jetonun bu alan adında ve `start_game` eylemi için alındığını da denetler. `.env.example` içindekiler Cloudflare'in her zaman geçen test anahtarlarıdır, yalnızca yerelde kullan. Test anahtarlarında alan adı ve eylem denetlenmez. Cloudflare'e ulaşılamazsa oyun başlatılamaz (503).

## Günde bir oyun

Tarayıcı ilk açılışta rastgele bir oyuncu kimliği üretip `localStorage`'da saklar ve oyunu başlatırken gönderir. Arka uç her kimliğe o sözcük için tek oyun verir. Oyun sürerken yeniden başlatılırsa aynı oyun, açılmış ipuçları ve tahminlerle kaldığı yerden sürer. Oyun bittikten sonra yeni oyun yerine sonuç gösterilir. Sonuç ekranında oyuncu o sözcüğü oynayan herkesle karşılaştırılır: oyuncu sayısı, bulma oranı, ortalamalar, puan dağılımı ve oyuncunun kaç kişiyi geçtiği. Bu sayılar her biten oyunla birikerek güncellenir, veritabanı yoktur. Kayıtlar ve istatistikler bellekte durur, yeni sözcükle ya da arka uç yeniden başlayınca sıfırlanır. Kimlik tarayıcı verisiyle silinebildiği için bu bir hile engeli değil, dürüst oyuncu için bir kuraldır.

## Önbellek ve CDN

nginx ön yüz dosyalarını önbellek başlıklarıyla sunar: HTML, JS ve CSS 5 dakika, yazı tipleri ve sesler 30 gün. `/api/` yanıtları hiç önbelleğe alınmaz. Önüne bir CDN (ör. Cloudflare) konursa nginx'e `set_real_ip_from` ve `real_ip_header` eklenmeli, yoksa bütün oyuncular CDN'in IP'si sayılır.

## Günün sözcüğünü yükleme

`/admin/` sayfasını aç, `ADMIN_TOKEN` parolasını gir, formu doldurup yayınla. `ADMIN_TOKEN` ayarlı değilse yükleme kapalıdır. Parola düz metin gittiği için canlıda HTTPS kullan.

- Sözcüğün işlenmiş hali yüklenirken hazırlanır ve bellekte durur, tahminlerde yeniden hesaplanmaz.
- Yayınlamadan önce "Deneme tahminleri" bölümüne oyuncuların yazabileceği tahminleri yazıp "Dene"ye bas. Sayfa hangilerinin yanlış değerlendirildiğini gösterir, yanlış değerlendirileni tek tıkla doğru anlatımlara ya da yanlış anlamlara taşırsın. `.env` içinde `ANTHROPIC_API_KEY` ayarlıysa "Yapay zekâya yazdır" bu tahminleri Claude'a (`claude-opus-5-5`) yazdırır, sözcüğün tanımı ve ipuçları Anthropic'e gider. Anahtar yoksa tahminler elle yazılır.
- Kazanmaması gereken bir deneme tahmini eşiği geçerse o sözcüğün kazanma eşiği onun 0,02 üstüne çıkar (en çok 0,70). Eşik hiç düşmez: örnek tahminlerle yapılan denemede eşiği düşürmek doğru tahminleri artırdı ama yanlış tahminlerin %2-3'ünü de kazandırdı.
- Zıt anlamlar ve yanlış anlamlar ayrı listelerdir. İkisi de kazanmayı zorlaştırır. Tahmin bunlardan birine doğru anlamlardan daha yakınsa oyuncu "Tersini söylüyor gibisin" ya da "Konu yakın, anlam başka" notunu görür.
- Yeni sözcük hemen oyuna girer, önceki sözcük bellekten ve diskten silinir. O sırada süren oyunlar kendi sözcükleriyle biter.
- Sözcük yüklendikten 24 saat sonra kendiliğinden silinir. Yenisi yüklenene kadar oyun başlatılamaz.
- Sunucu yeniden başlarsa sözcük `WORD_FILE` dosyasından okunup yeniden hazırlanır. Bu dosya depoya girmez.
- Tahminlerin vektörleri en çok 1000 tahminlik bir önbellekte tutulur, az kullanılanlar yenilerine yer açar. Aynı anda gelen aynı tahmin modele bir kez gider. Sözcük değişince ya da süresi dolunca önbellek boşaltılır.

Doğruluğu örnek sözcüklerle ölçmek için TEI açıkken `backend` klasöründe:

    cargo run --release --bin evaluate_guesses
    cargo test -- --ignored

`evaluate_guesses -- --plain-letters` aynı tahminleri Türkçe harfleri silinmiş olarak ölçer.

Hiç Türkçe harf (ç, ğ, ı, ö, ş, ü) içermeyen bir tahmin İngilizce klavyeyle yazılmış sayılır. Arka uç her sözcüğü en sık kullanılan Türkçe yazımına çevirir ("geldigini" → "geldiğini"). Sözcük listesi `backend/resources/turkish_words.txt`, [FrequencyWords](https://github.com/hermitdave/FrequencyWords) OpenSubtitles 2018 Türkçe listesinin ilk 200 bin sözcüğünden üretildi (CC BY-SA 4.0). Örnek tahminlerde harfler silinince doğru tahminlerin kazanma oranı %76'dan %49'a düşüyordu, çeviriyle %75 oldu. Sözcüğün kendisi de Türkçe harfsiz yazılırsa yakalanır.

Model derlenirken `ai/quantize.py` ile int8'e indirilir. Ağırlık çarpımları `ai/calibration_texts.txt` içindeki örnek tanımlarla önceden ölçeklenir (statik nicemleme). Ölçekler sabit olduğu için bir tahminin skoru aynı anda işlenen diğer tahminlere bağlı değildir. Dinamik nicemleme daha basittir ama TEI istekleri toplu işlediği için skorlar yüke göre oynuyordu. Model ya da nicemleme değişirse kazanma eşiği `evaluate_guesses` ile yeniden ayarlanmalı.

Model en çok 75 parça (token) okur. Arka uç TEI'ye metni kesmemesini söyler: daha uzun bir tahmin sessizce kırpılmak yerine oyuncuya "çok uzun" diye geri döner, daha uzun bir tanım da yönetim sayfasında reddedilir. Olağan Türkçe metinde 200 karakter yaklaşık 40 parça eder.

Oyun ayarları: kazanma eşiği ve önbellek boyutu `backend/src/similarity.rs`, süre ve puan `backend/src/game.rs`, sözcüğün ömrü `backend/src/daily_word.rs`, istek sınırları `backend/src/rate_limit.rs`.

## İstek sınırı

Oyun başlatma, tahmin, ipucu, sonuç ve yönetim istekleri IP başına sınırlı. Sınır aşılınca sunucu 429 ve `Retry-After` döner.

Ayrıca her oyunda iki tahmin arasında en az `GUESS_INTERVAL_SECONDS` (`backend/src/game.rs`, şimdilik 2 sn) beklenir. Ön yüz bu sürede tahmin düğmesini kapatır, arka uç erken gelen tahmini yine 429 ile reddeder. Süre, gerçek oyuncu verisi gelince ayarlanacak. Sayaçlar oyunlar gibi süreç belleğinde tutuluyor, bu yüzden arka uç tek kopya çalışır.

Arka uç oyuncunun IP'sini nginx'in eklediği `X-Real-IP` başlığından okur (`CLIENT_IP_HEADER`). Bu yüzden `backend` kapsayıcısının portu dışarıya açılmamalı, yoksa oyuncular IP'lerini taklit edebilir.
