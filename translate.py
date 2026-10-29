import json
import os

# Път до папката locales
base_path = r"C:\Users\Dell\Documents\GitHub\solidcraft\assets\locales"
en_file = os.path.join(base_path, "en.json")
bg_file = os.path.join(base_path, "pending", "bulgarian-batch-1.json")

# Зареждаме английския файл
with open(en_file, "r", encoding="utf-8") as f:
    en_data = json.load(f)

# Речник с преводи (за начало, само основни термини)
translations = {
    "Add Column": "Добави колона",
    "About SolidCraft": "Относно SolidCraft",
    "Add": "Добави",
    "Cancel": "Отказ",
    "OK": "OK",
    "Save": "Запази",
    "Open": "Отвори",
    "Close": "Затвори",
    "Delete": "Изтрий",
    "Edit": "Редактирай",
    "File": "Файл",
    "Help": "Помощ",
    "Settings": "Настройки",
    "Preferences": "Предпочитания",
    "Exit": "Изход",
    "Yes": "Да",
    "No": "Не",
    "Apply": "Приложи",
    "Reset": "Нулирай",
    "Search": "Търси",
    "Language": "Език",
    "English": "Английски",
    "Bulgarian": "Български",
    "Sketch": "Скица",
    "Extrude": "Екструдиране",
    "Revolve": "Ротация",
    "Constraint": "Ограничение",
    "Feature": "Елемент",
    "History": "История",
    "Parameters": "Параметри",
    "Dimension": "Размер",
    "Angle": "Ъгъл",
    "Length": "Дължина",
    "Width": "Широчина",
    "Height": "Височина",
    "Depth": "Дълбочина",
    "Radius": "Радиус",
    "Diameter": "Диаметър",
    "Volume": "Обем",
    "Area": "Площ",
    "Mass": "Маса",
    "Material": "Материал",
    "Export": "Експорт",
    "Import": "Импорт",
    "STEP": "STEP",
    "STL": "STL",
    "Mesh": "Мрежа",
    "3D": "3D",
    "View": "Изглед",
    "Zoom": "Мащаб",
    "Pan": "Плъзгане",
    "Rotate": "Завъртане",
    "Top": "Отгоре",
    "Bottom": "Отдолу",
    "Front": "Отпред",
    "Back": "Отзад",
    "Left": "Ляво",
    "Right": "Дясно",
    "Isometric": "Изометричен",
    "Perspective": "Перспектива",
    "Orthographic": "Ортогонален",
    "Grid": "Мрежа",
    "Snap": "Прилепване",
    "Constraint Solver": "Решаване на ограничения",
    "Profile": "Профил",
    "Plane": "Равнина",
    "Axis": "Ос",
    "Origin": "Начало",
    "Point": "Точка",
    "Line": "Линия",
    "Circle": "Окръжност",
    "Arc": "Дъга",
    "Rectangle": "Правоъгълник",
    "Polygon": "Многоъгълник",
    "Spline": "Сплайн",
    "Fillet": "Заобляне",
    "Chamfer": "Фаска",
    "Shell": "Черупка",
    "Offset": "Отместване",
    "Boolean": "Булева операция",
    "Union": "Обединение",
    "Subtract": "Изваждане",
    "Intersect": "Сечение",
    "Error": "Грешка",
    "Warning": "Предупреждение",
    "Info": "Информация",
    "Success": "Успех",
    "Loading": "Зареждане",
    "Saving": "Запазване",
    "Processing": "Обработка",
    "Ready": "Готово",
    "Not supported yet": "Все още не се поддържа",
    "Crash report": "Доклад за срив",
    "Autosave": "Автоматично запазване",
    "Recovery": "Възстановяване",
    "Reopen": "Отвори отново",
    "Recent files": "Скорошни файлове",
    "New": "Нов",
    "New document": "Нов документ",
    "Open document": "Отвори документ",
    "Save as": "Запази като",
    "Properties": "Свойства",
    "Design properties": "Свойства на дизайна",
    "Keyboard shortcuts": "Клавишни комбинации",
    "Navigation": "Навигация",
    "Graphics": "Графика",
    "Interface": "Интерфейс",
    "Design": "Дизайн",
    "General": "Общи",
    "Tools": "Инструменти",
    "Extend": "Разширяване",
    "Assistant": "Асистент",
    "Plugins": "Плъгини",
    "Slicer": "Слайсер",
}

# Създаваме нов обект с преводи
bg_data = {"bg": {}}

# Обхождаме всички ключове в en.json
for key, value in en_data.items():
    if key in translations:
        bg_data["bg"][key] = translations[key]
    else:
        # Ако няма превод, оставяме английския текст (за да не се чупи)
        bg_data["bg"][key] = value

# Записваме файла
with open(bg_file, "w", encoding="utf-8") as f:
    json.dump(bg_data, f, ensure_ascii=False, indent=2)

print(f"Готово! Създаден е файл: {bg_file}")
print(f"Преведени ключове: {len(translations)}")
print(f"Общо ключове: {len(en_data)}")