# Gelecek işler

## Anlam parçaları listesi

Bugün tahmin tanımın bütünüyle karşılaştırılıyor ve oyuncu tek bir yüzde görüyor. Kaçırılan doğru tahminlerin çoğu somut bir örneği anlatıyor, tanım ise soyut. Cümle vektöründe de ortak konu baskın çıkıyor.

Öneri: yönetici her sözcük için 2-4 anlam parçası yazar. Örneğin önözlem için "güzel bir an", "henüz bitmemiş", "şimdiden özlemek".

- Tahmin her parçayla ayrı karşılaştırılır (çıkarım modeli ya da benzerlik).
- Oyuncu "3 parçadan 2'sini buldun" gibi parça parça geri bildirim alır.
- Zıt anlam, bir parçanın tersi olarak yakalanır ("şimdiden" yerine "geçmişte").
- Bütün parçalar bulununca tahmin kazanır. Mevcut eşik yedek olarak kalabilir.

Önce çevrimdışı dene: 14 örnek sözcük için parça listeleri yaz, `backend/tools/sample_guesses.json` ile bugünkü yöntemle karşılaştır. Ölçüt: sıfır yanlış kabulde doğru oranı ve parça geri bildiriminin doğruluğu.

Açık sorular: parçalar oyuncuya baştan gösterilir mi (yalnızca sayısı), parça sayısı puanı etkiler mi, kategori ipucuyla çakışır mı.
