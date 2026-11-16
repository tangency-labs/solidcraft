import json
import os

with open('assets/locales/en.json', 'r', encoding='utf-8') as f:
    en = json.load(f)

already_translated = set()
# Търсим във ВСИЧКИ файлове в папката, за да сме сигурни, че нищо не се губи
for filename in os.listdir('assets/locales/pending'):
    if filename.endswith('.json') and ('bulgarian' in filename or 'bg' in filename):
        path = os.path.join('assets/locales/pending', filename)
        try:
            with open(path, 'r', encoding='utf-8') as f:
                data = json.load(f)
                keys = data.get("bg", data).keys()
                already_translated.update(keys)
        except:
            pass

# Вземаме следващите 250 ключа
missing = {k: v for k, v in en.items() if k not in already_translated}
next_250 = dict(list(missing.items())[:250])

# Записваме ги в специален файл САМО за превод (без да пипаме старите!)
output_path = 'assets/locales/pending/batch-4-ENGLISH-TO-TRANSLATE.json'
with open(output_path, 'w', encoding='utf-8') as f:
    json.dump({"bg": next_250}, f, ensure_ascii=False, indent=2)

print(f"Общо преведени: {len(already_translated)}")
print(f"Остават: {len(missing)}")
print(f"Моля, преведете файла: {output_path}")