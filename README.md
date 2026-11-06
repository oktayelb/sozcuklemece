sozcuklemece


Tarayıcılar üzerinden oynanılan günlük oyunların arttığı bu dönemde yeni ve bambaşka bir bulmaca

BU günlük bulmacada kullanıclar verilen karşılarına çıkan yepyeni türetme sözcüğün anlamını tahmin etmeye çalışıyorlar.
oyun şöyle olacak. 3 dakikalık bir sayaç ve bu sürede bulmaya çalışan bir oyuncu var. Ne kadar erken bulursa o kadar iyi, kullanılan ipuçları kazandığı puanı azaltıyor.
İpuçlar:
    kelimenin kökündeki sözcük(ler), ek(ler) 
    cümlede kullanımı
    Kategori ve ilgili olduğu alan (bilim, yemek,siyaset, his)
her gün 1 ya da  yeni sözcükle yarışma oluyor.


Buradaki asıl olay şu, Sözcüğün tanımı 1 tane, ama kullanıcıların yazabilecekleri tanımlar sonsuz tane olduğu için iki cümleyi kıyaslayabilecek bir yönteme ihtiyacımız var.
Burada aynı zamanda çokça kullanıcının siteye gireceği kalabalık zamanları da düşündüğümüz için bu yöntemin çok hızlı olması gerek. Bunun için benim aklımaasıl tanım ile kullanıcı tahminini word2vec'ten geçirip, her birini kendi vektörleri ile topladıktan sonra uzaklıklarını hesaplamak geldi. Bu fikir nasıl sence? Bundan daha iyi ve hız/doğruluk performansı olarak daha iy ibir yöntem geliyorsa aklına söyle
 FastAPI ile yapmanın iyi olacağını düşünüyorum, üyelik gerekmiyor ve yz kullanacağız, bu yüzden hız çok önemli.
 

## Çalıştırma

    uv sync
    uv run uvicorn backend.main:app --port 8000

Ardından http://127.0.0.1:8000 adresini aç. İlk açılışta model (yaklaşık 450 MB) indirilir.

Testler: `uv run pytest`

## Yeni sözcük ekleme

`backend/puzzles.json` dosyasına ekle:

- `definition`: oyun sonunda gösterilen tanım
- `also_accepted`: oyuncuların yazabileceği kısa doğru anlatımlar
- `wrong_meanings`: akla gelen yanlış anlamlar, tanımın zıttı ve fazla genel anlatımlar
- `inflected_stems` (isteğe bağlı): sözcüğün çekimli kökü, ör. `evimse`, `oyalanc`

Sonra doğrulukla ilgili hatalı kararları gör: `uv run python -m tools.evaluate_guesses`

Oyun ayarları: kazanma eşiği `backend/similarity.py`, süre ve puan `backend/game.py`, günlük sözcük sayısı `backend/puzzles.py`.
