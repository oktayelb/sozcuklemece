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

    uv sync
    ADMIN_TOKEN=uzun-rastgele-bir-parola uv run uvicorn backend.main:app --port 8000

Ardından http://127.0.0.1:8000 adresini aç. İlk açılışta model (yaklaşık 450 MB) indirilir.

Testler: `uv run pytest`

## Docker ile çalıştırma

    ADMIN_TOKEN=uzun-rastgele-bir-parola docker compose up -d --build

Model imajın içine indirilir, kapsayıcı açılışta internete çıkmaz. Günün sözcüğü `word-data` biriminde durur, yeniden başlatmada kaybolmaz. Ters vekil ayrı bir kapsayıcıdaysa `FORWARDED_ALLOW_IPS` ortam değişkenine vekilin IP'sini ver.

## Günün sözcüğünü yükleme

http://127.0.0.1:8000/admin/ adresini aç, `ADMIN_TOKEN` parolasını gir, formu doldurup yayınla. `ADMIN_TOKEN` ayarlı değilse yükleme kapalıdır. Parola düz metin gittiği için canlıda HTTPS kullan.

- Sözcüğün işlenmiş hali yüklenirken hazırlanır ve bellekte durur, tahminlerde yeniden hesaplanmaz.
- Yeni sözcük hemen oyuna girer, önceki sözcük bellekten ve diskten silinir. O sırada süren oyunlar kendi sözcükleriyle biter.
- Sözcük yüklendikten 24 saat sonra kendiliğinden silinir. Yenisi yüklenene kadar oyun başlatılamaz.
- Sunucu yeniden başlarsa sözcük `data/current_word.json` dosyasından okunup yeniden hazırlanır. Bu dosya depoya girmez.

Doğruluğu örnek sözcüklerle ölçmek için: `uv run python -m tools.evaluate_guesses`

Oyun ayarları: kazanma eşiği `backend/similarity.py`, süre ve puan `backend/game.py`, sözcüğün ömrü `backend/daily_word.py`, istek sınırları `backend/rate_limit.py`.

## İstek sınırı

Oyun başlatma, tahmin ve yönetim istekleri IP başına sınırlı. Sınır aşılınca sunucu 429 ve `Retry-After` döner. Sayaçlar oyunlar gibi süreç belleğinde tutuluyor, bu yüzden tek süreçle çalışır.

Ters vekil (nginx vb.) başka bir makinedeyse uvicorn'a `--forwarded-allow-ips <vekil-ip>` ver, yoksa bütün oyuncular tek IP sayılır.
