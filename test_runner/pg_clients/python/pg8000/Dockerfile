FROM python:3.11
WORKDIR /source

COPY . .

RUN python3 -m pip install --no-cache-dir -r requirements.txt

CMD ["python3", "pg8000_example.py"]
