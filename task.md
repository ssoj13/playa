1. trim layers out, extend their length - now we can just trim layers in. If trim extended - we should display the last image from that end - is it start or end.
3. timeline bookmarks: shift-number - set, number - go to, per comp (sets in parent comp)
4. В layer src_len всегда должен браться из children comp. если там изменился - то и в layer изменился
5. подумать как реализовать layer picking.
6. Чтение EDL или OpenTimelineIO как входного файла
7. Изучить возможность использования OCIO/OIIO
8. проверить куда делась полосатая закраска баров в таймлайне, она у нас была какое-то время назад.
9. встроить REST API, чтобы можно было удалённо работать с плеером, опрашивать статус, значения переменных, менять кадры, загружать сиквенсы, посылать любые сообщения.
10. изучить вопрос добавления таймкода

Добавить в атрибуты атрибутов атрибут "order" - float attribute который указывает на порядок атрибутов. В AE они должны рендериться в этом порядке.
Просмотреть все атрибуты, и сгруппировать в логические группы. Скажем все translation/rotate/scale атрибуты должны идти вместе, а pivot должен быть сразу после них.
